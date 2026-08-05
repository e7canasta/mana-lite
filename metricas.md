**Guía de métricas de ingest**

---

### La línea base

```
ingest: 0.6 Hz — 3 keyframes in 5s | decode 14ms avg | cycles 48 | pframes:97 timeouts:48
```

| Campo | Qué significa | Bueno | Malo |
|-------|---------------|-------|------|
| `0.6 Hz` | Keyframes por segundo | ≥0.3 (clínico) | <0.1 (stream muy lento o congelado) |
| `3 keyframes in 5s` | Total en la ventana | Estable entre reportes | Cae a 0 → cámara muerta |
| `decode 14ms avg` | Tiempo de decodificación H.264→RGB por frame | <30ms (1080p) | >100ms (CPU saturada) |
| `cycles` | Vueltas del superloop en la ventana | ~10× keyframes | >20× → mucho polling vacío |
| `pframes:N` | P-frames descartados en la ventana | ~50-100 (normal, hay muchos) | 0 → stream sin p-frames (raro) |
| `timeouts:N` | Polls RTSP sin respuesta | = cycles (normal, es polling) | Sube drásticamente → red lenta |

### Flags de alarma

| Flag | Significado | Qué hacer |
|------|-------------|-----------|
| `dup:N` | Mismos bytes H264 repetidos | Cámara congelada enviando el mismo frame. Verificar fuente. |
| `reconnect:N` | Reconexión RTSP | Se cayó la cámara o red. `reconnect>2` en 5s es grave. |
| `ssrc:N` | Cambio de SSRC | El stream se reinició (cámara reboot, cambio de encoder). |
| `rtp:N` | Errores de paquete RTP | Red con pérdida de paquetes. Si >10 reconsiderar TCP. |

### Escenarios típicos

**1. Todo normal**

```
ingest: 0.6 Hz — 3 keyframes in 5s | decode 14ms avg | cycles 48 | pframes:97 timeouts:48
```

~2-3 keyframes cada 5s, decode estable, sin flags de alarma. La cámara está bien.

**2. Cámara lenta (0.2 Hz)**

```
ingest: 0.2 Hz — 1 keyframes in 5s | decode 6ms avg | cycles 50 | pframes:49 timeouts:50
```

Solo 1 keyframe por ventana. Puede ser GOP largo (>10s) o cámara con bitrate bajo. Verificar configuración de la cámara (IDR interval).

**3. Stream congelado**

```
ingest: 0.0 Hz — 0 keyframes in 5s | decode 0ms avg | cycles 51 | pframes:0 dup:0 timeouts:51
```

0 keyframes, eventualmente dispara `HealthTransition::Blind` y FSM → `blind`. Cámara caída o cable desconectado.

**4. Keyframes duplicados**

```
ingest: 0.4 Hz — 2 keyframes in 5s | decode 14ms avg | cycles 46 | pframes:90 dup:3 timeouts:46
```

`dup:3` → la cámara envió 3 keyframes idénticos. Si es persistente, la escena no cambia (habitación vacía) o la cámara está congelada en un frame.

**5. Reconexión**

```
ingest: 0.2 Hz — 1 keyframes in 5s | decode 18ms avg | cycles 42 | pframes:12 timeouts:2 reconnect:3
```

3 reconexiones en 5s → cámara intermitente. Verificar cable/power/red.

**6. Decode lento**

```
ingest: 0.6 Hz — 3 keyframes in 5s | decode 85ms avg | cycles 44 | pframes:97 timeouts:44
```

85ms por frame → CPU saturada. Posible con 4K o muchos modelos corriendo. Considerar `imgsz=320` o menos modelos.

### Relación cycles vs timeouts

`timeouts` ≈ `cycles` es **normal** — cada ciclo del loop hace un poll RTSP que da timeout (50ms) cuando no hay frame nuevo. Es el mecanismo de polling.

Si `cycles` >> `timeouts` → hay algo raro. Significa que el loop está girando más rápido de lo esperado (sin sleep en demo mode, por ejemplo).

### El reporte de 12s

```
ingest: 0.2 Hz — 3 keyframes in 12s | decode 15ms avg | cycles 14 | pframes:245 timeouts:14
```

Una ventana de 12s en vez de 5s → el reporte anterior no se emitió a tiempo (posible bloqueo en inferencia o viz). Los 3 keyframes en 12s dan 0.25 Hz real, consistente con el stream. 245 p-frames en 12s ≈ 20 p-frames/segundo → stream 25fps con GOP ~5s (normal).
