//! `Slot<T>`: acoplamiento por muestra entre etapas de distinta tasa.
//!
//! La primitiva de ADR-034. La regla que decide si un borde es slot o cola:
//!
//! > Si perder el dato viejo es **correcto**, es una muestra: va en slot.
//! > Si perderlo es un **bug**, es un evento: va en cola.
//!
//! Un frame, una imagen de proceso o el estado del tracker son muestras: la
//! anterior perdió su valor en el momento en que llegó la siguiente. Aplicarles
//! contrapresión es un error de categoría — la contrapresión sirve cuando el
//! productor *puede* ir más lento, y la cámara no puede: va a entregar el
//! próximo keyframe llegue o no llegue el anterior.

use std::sync::{Condvar, Mutex};

/// Buffer de un elemento con semántica "el último gana".
///
/// `put` sobrescribe siempre y cuenta lo pisado. Ninguna de las dos puntas
/// bloquea nunca por contrapresión: la latencia queda acotada por construcción
/// a un elemento de antigüedad.
pub struct Slot<T> {
    state: Mutex<SlotState<T>>,
    /// Sólo para que un consumidor pueda dormir en vez de girar en vacío.
    /// No es contrapresión: el productor jamás espera en esta variable.
    ready: Condvar,
}

struct SlotState<T> {
    value: Option<T>,
    /// Muestras pisadas sin haber sido consumidas. Es la señal de que el
    /// consumidor no da abasto, y un descarte que no se cuenta es
    /// indistinguible de que el sistema ande bien.
    overwritten: u64,
    /// Un productor cerrado despierta a quien esté esperando, para que la
    /// etapa de abajo termine en vez de colgarse.
    closed: bool,
}

impl<T> Slot<T> {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SlotState {
                value: None,
                overwritten: 0,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    /// Publica una muestra, pisando la anterior si nadie la consumió.
    ///
    /// Nunca bloquea y nunca falla: si la etapa de abajo se atrasó, lo correcto
    /// es que vea la muestra fresca y no la vieja.
    pub fn put(&self, value: T) {
        let mut state = self.lock();
        if state.value.is_some() {
            state.overwritten += 1;
        }
        state.value = Some(value);
        drop(state);
        self.ready.notify_one();
    }

    /// Consume la muestra pendiente si la hay. **Nunca bloquea.**
    ///
    /// Es la única forma que puede usar el lazo de control: un `take` que
    /// pudiera esperar volvería a acoplar la cadencia a la etapa de arriba, que
    /// es exactamente lo que este tipo existe para impedir.
    pub fn take(&self) -> Option<T> {
        self.lock().value.take()
    }

    /// Espera hasta que haya una muestra y la consume. `None` si el productor
    /// cerró el slot.
    ///
    /// Para consumidores cuyo trabajo *es* la muestra —el hilo de percepción no
    /// tiene nada que hacer sin un keyframe— y que por lo tanto pueden dormir
    /// sin acoplar a nadie. Nunca la use el lazo de control.
    pub fn take_blocking(&self) -> Option<T> {
        let mut state = self.lock();
        loop {
            if let Some(value) = state.value.take() {
                return Some(value);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Muestras pisadas desde la última consulta, y las pone en cero.
    ///
    /// Se publica en el reporte de métricas: un borde sin instrumentar es un
    /// borde sobre el que no se puede razonar cuando algo va mal.
    pub fn drain_overwritten(&self) -> u64 {
        std::mem::take(&mut self.lock().overwritten)
    }

    /// Cierra el slot y despierta a quien espere, para que la etapa de abajo
    /// pueda terminar en un apagado ordenado.
    pub fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }

    /// Un `Mutex` envenenado significa que otra etapa entró en pánico. El slot
    /// no es dueño de esa política —el watchdog de pánicos lo es— y quedarse
    /// con el dato es siempre mejor que propagar el pánico a una etapa sana.
    fn lock(&self) -> std::sync::MutexGuard<'_, SlotState<T>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn the_newest_sample_wins_and_the_old_one_is_counted() {
        let slot = Slot::new();
        slot.put(1);
        slot.put(2);
        slot.put(3);

        assert_eq!(slot.take(), Some(3), "el consumidor ve lo último, no lo viejo");
        assert_eq!(slot.drain_overwritten(), 2);
        assert_eq!(slot.drain_overwritten(), 0, "el contador se drena");
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn consuming_in_time_overwrites_nothing() {
        let slot = Slot::new();
        for value in 0..10 {
            slot.put(value);
            assert_eq!(slot.take(), Some(value));
        }
        assert_eq!(slot.drain_overwritten(), 0);
    }

    /// La propiedad de la que depende el lazo de control: `take` contesta ya
    /// mismo, haya o no muestra. Si esto se rompiera, la cadencia volvería a
    /// depender de la etapa de arriba.
    #[test]
    fn take_never_blocks_an_empty_slot() {
        let slot: Slot<u32> = Slot::new();
        assert_eq!(slot.take(), None);
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn a_blocking_consumer_wakes_on_the_next_sample() {
        let slot = Arc::new(Slot::new());
        let consumer = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || slot.take_blocking())
        };
        std::thread::sleep(Duration::from_millis(20));
        slot.put(42);
        assert_eq!(consumer.join().unwrap(), Some(42));
    }

    /// Un apagado no puede dejar a la etapa de abajo dormida para siempre.
    #[test]
    fn closing_releases_a_waiting_consumer() {
        let slot: Arc<Slot<u32>> = Arc::new(Slot::new());
        let consumer = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || slot.take_blocking())
        };
        std::thread::sleep(Duration::from_millis(20));
        slot.close();
        assert_eq!(consumer.join().unwrap(), None);
    }

    /// El productor no puede quedar esperando a un consumidor lento: es la
    /// diferencia entre esto y una cola, y es la razón de ser del tipo.
    #[test]
    fn a_stalled_consumer_never_holds_up_the_producer() {
        let slot = Arc::new(Slot::new());
        let producer = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || {
                for value in 0..10_000u32 {
                    slot.put(value);
                }
            })
        };
        producer.join().expect("el productor termina sin consumidor");
        assert_eq!(slot.take(), Some(9_999));
        assert_eq!(slot.drain_overwritten(), 9_999);
    }
}
