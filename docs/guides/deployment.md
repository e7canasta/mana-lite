# Guia de despliegue

Esta guia describe el despliegue de `mana-lite` en un equipo Linux dedicado a
una camara. El despliegue recomendado es un binario Rust supervisado por
`systemd`, con `go2rtc` como fuente RTSP local y los pesos ONNX instalados fuera
del control de versiones.

`mana-lite` no incluye la camara, `go2rtc`, Rerun ni los pesos de los modelos.
Son dependencias del equipo donde se ejecuta.

## 1. Arquitectura desplegada

```text
camara -> go2rtc:8554 -> mana-lite -> logs/*.jsonl
                              |
                              +-> Rerun:9876 (opcional)
```

El proceso principal es `mana-lite`. Para una instalacion de una sola camara
no hace falta desplegar otros procesos de Mana OS ni configurar IPC.

## 2. Requisitos

- Linux x86_64 o ARM64.
- Rust `1.89` o posterior para compilar desde fuente.
- FFmpeg y sus headers de desarrollo, requeridos por `ffmpeg-next`.
- `git`, `pkg-config`, un compilador C y `systemd`.
- `go2rtc` escuchando en el puerto `8554` y publicando un stream RTSP.
- Los pesos ONNX aprobados para el catalogo usado por la configuracion.
- Rerun solo si `[viz].enabled = true`.

En Debian/Ubuntu, una base habitual para compilar es:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config \
  libavcodec-dev libavformat-dev libavutil-dev libswresample-dev \
  libavfilter-dev libavdevice-dev
```

Instalar Rust con `rustup` si el equipo no lo tiene:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install stable
```

## 3. Preparar el checkout

El crate depende por ruta de `mana-inference`. Los repositorios deben ser
hermanos:

```text
/opt/mana-lite-workspace/
├── mana-lite/
└── inference/
```

Ejemplo de instalacion:

```sh
sudo mkdir -p /opt/mana-lite-workspace
sudo chown "$USER:" /opt/mana-lite-workspace
git clone https://github.com/e7canasta/mana-lite.git \
  /opt/mana-lite-workspace/mana-lite
git clone https://github.com/e7canasta/mana-inference.git \
  /opt/mana-lite-workspace/inference
cd /opt/mana-lite-workspace/mana-lite
```

Para desplegar una version concreta, usar un tag o un commit revisado en ambos
repositorios, en lugar de compilar siempre desde `master`.

## 4. Instalar los pesos ONNX

Los pesos no estan en Git. Copiarlos desde el almacenamiento de artifacts
aprobado al checkout, conservando las rutas del catalogo:

```sh
cd /opt/mana-lite-workspace/mana-lite
test -f tools/model-tools/artifacts/yolo26-fp16/yolo26s-fp16-320.onnx
test -f tools/model-tools/artifacts/yoloface-fp16/yolov12s-face-fp16-320.onnx
```

El catalogo completo tambien puede requerir pose, segmentacion y depth:

```sh
test -f tools/model-tools/artifacts/yolo26-fp16/yolo26s-pose-fp16-320.onnx
test -f tools/model-tools/artifacts/yolo26-fp16/yolo26s-seg-fp16-320.onnx
```

Si los artifacts viven en otro checkout, usar `MANA_MODELS_HOME`. Esta variable
reancla las rutas relativas del catalogo sin modificar los archivos versionados:

```sh
export MANA_MODELS_HOME=/srv/mana-models/mana-lite
```

## 5. Configurar la instalacion

Partir de `config/mana.toml`. Como minimo, revisar:

```toml
[source]
url = "rtsp://127.0.0.1:8554/home2"
username = "admin"
password = ""
transport = "tcp"

[viz]
enabled = true
rerun_addr = "127.0.0.1:9876"
```

No guardar contrasenas reales en un archivo versionado. Las variables de
entorno permiten sobrescribir los valores de despliegue mas sensibles:

```sh
MANA_SOURCE_URL=rtsp://127.0.0.1:8554/home2
MANA_SOURCE_USERNAME=admin
MANA_SOURCE_PASSWORD='secreto'
MANA_RERUN_ADDR=127.0.0.1:9876
MANA_SAVE_DIR=/var/lib/mana-lite/logs
MANA_MODELS_HOME=/opt/mana-lite-workspace/mana-lite
```

Los paths relativos de la configuracion se resuelven desde el checkout. Por
eso el servicio debe declarar `WorkingDirectory` y no ejecutarse desde `/`.

## 6. Compilar y verificar

Compilar antes de medir o activar el servicio:

```sh
cd /opt/mana-lite-workspace/mana-lite
cargo test --workspace --release
cargo build --release --bin mana-lite
```

Probar primero la fuente RTSP y luego un arranque controlado:

```sh
ffmpeg -rtsp_transport tcp -i rtsp://127.0.0.1:8554/home2 \
  -frames:v 1 -f null -
timeout 20 cargo run --release -- --config config/mana.toml
```

La salida esperada incluye el arranque del proceso, la carga de los modelos y
la publicacion periodica de JSONL. Revisar que no haya errores de `FileNotFound`,
decode, reconexion continua o modelo incompatible.

## 7. Ejecutar con systemd

Crear `/etc/mana-lite/mana-lite.env` con permisos restringidos:

```sh
sudo install -d -m 0750 /etc/mana-lite /var/lib/mana-lite/logs
sudo install -m 0600 /dev/null /etc/mana-lite/mana-lite.env
sudoedit /etc/mana-lite/mana-lite.env
```

Contenido minimo del archivo de entorno:

```sh
MANA_MODELS_HOME=/opt/mana-lite-workspace/mana-lite
MANA_SOURCE_URL=rtsp://127.0.0.1:8554/home2
MANA_SOURCE_USERNAME=admin
MANA_SOURCE_PASSWORD=
MANA_SAVE_DIR=/var/lib/mana-lite/logs
MANA_VIZ_ENABLED=false
```

Crear `/etc/systemd/system/mana-lite.service`:

```ini
[Unit]
Description=Mana Lite clinical vision pipeline
After=network-online.target go2rtc.service
Wants=network-online.target

[Service]
Type=simple
User=mana
Group=mana
WorkingDirectory=/opt/mana-lite-workspace/mana-lite
EnvironmentFile=/etc/mana-lite/mana-lite.env
ExecStart=/opt/mana-lite-workspace/mana-lite/target/release/mana-lite --config config/mana.toml
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

El usuario del servicio debe poder leer el checkout y escribir el directorio de
salida:

```sh
sudo useradd --system --home /nonexistent --shell /usr/sbin/nologin mana
sudo chown -R mana:mana /opt/mana-lite-workspace /var/lib/mana-lite
sudo systemctl daemon-reload
sudo systemctl enable --now mana-lite.service
```

Verificar estado y logs:

```sh
systemctl status mana-lite.service
journalctl -u mana-lite.service -f
ls -lh /var/lib/mana-lite/logs
```

## 8. Visualizacion con Rerun

La visualizacion es opcional y no debe bloquear el lazo de control. Para verla
en el mismo equipo:

```sh
rerun --port 9876
```

Luego establecer `MANA_VIZ_ENABLED=true` y `MANA_RERUN_ADDR=127.0.0.1:9876` en
el environment file, y reiniciar:

```sh
sudo systemctl restart mana-lite.service
```

Si Rerun corre en otra maquina, usar la direccion alcanzable desde el equipo de
Mana y abrir solo ese puerto en la red de administracion.

## 9. Actualizar una version

Hacer las actualizaciones con el servicio detenido y conservar los logs:

```sh
sudo systemctl stop mana-lite.service
cd /opt/mana-lite-workspace/mana-lite
git fetch --tags origin
git checkout <version-revisada>
cd ../inference
git fetch --tags origin
git checkout <version-compatible>
cd ../mana-lite
cargo test --workspace --release
cargo build --release --bin mana-lite
sudo systemctl start mana-lite.service
```

Confirmar despues del arranque:

```sh
systemctl is-active mana-lite.service
journalctl -u mana-lite.service -n 100 --no-pager
```

## 10. Diagnostico rapido

| Sintoma | Revisar |
|---|---|
| `No such file or directory` al cargar un modelo | `MANA_MODELS_HOME` y las rutas bajo `tools/model-tools/artifacts/` |
| No conecta a RTSP | `systemctl status go2rtc`, puerto `8554`, URL y credenciales |
| Reconecta continuamente | logs de `mana-lite`, disponibilidad de keyframes I y transporte TCP |
| No aparecen frames en Rerun | `MANA_VIZ_ENABLED`, `MANA_RERUN_ADDR`, firewall y puerto `9876` |
| El servicio arranca y termina | `journalctl -u mana-lite.service -b` y permisos del checkout |
| No se escriben JSONL | `MANA_SAVE_DIR`, permisos de `/var/lib/mana-lite/logs` y `jsonl_level` |

Ante cualquier cambio de codigo o configuracion, repetir como minimo
`cargo test --workspace --release`, una prueba RTSP y un arranque controlado
antes de volver a activar el servicio.
