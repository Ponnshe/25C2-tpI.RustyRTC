# Roadmap: Integración de Métricas RTCP

## Visión General

Este documento describe una hoja de ruta detallada para integrar y utilizar las métricas proporcionadas por los paquetes RTCP (RTP Control Protocol) en la aplicación `rustyrtc`. El objetivo es mejorar la monitorización de la calidad de la sesión de WebRTC, proporcionando información en tiempo real sobre la pérdida de paquetes, el jitter y el Round-Trip Time (RTT).

## Estado Actual

La implementación actual de `rustyrtc` tiene un soporte básico para RTCP:

- **Decodificación de Paquetes:** La aplicación es capaz de decodificar paquetes RTCP compuestos, incluyendo `Sender Reports` (SR) y `Receiver Reports` (RR).
- **Generación de Reportes:** La aplicación genera y envía `Receiver Reports` (RR) con estadísticas básicas sobre los streams de recepción.
- **Manejo de Reportes:** Los `Report Blocks` recibidos dentro de los paquetes SR y RR se enrutan al `RtpSendStream` correspondiente.
- **Métricas no utilizadas:** El método `on_report_block` en `RtpSendStream` está actualmente vacío, lo que significa que las métricas de calidad de red (pérdida de paquetes, jitter, RTT) se reciben pero no se procesan ni se utilizan.

## Arquitectura Propuesta

Para integrar completamente las métricas de RTCP, proponemos las siguientes modificaciones en la arquitectura:

1.  **`RtpSendStream`:** Esta estructura se extenderá para procesar los `Report Blocks` y calcular métricas clave como:
    -   **Fracción de paquetes perdidos:** Utilizando el campo `fraction_lost`.
    -   **Jitter:** Utilizando el campo `interarrival_jitter`.
    -   **Round-Trip Time (RTT):** Calculado a partir de los campos `last_sr` (LSR) y `delay_since_last_sr` (DLSR).

2.  **`RtpSession`:** Se añadirá un mecanismo para que `RtpSession` pueda consultar periódicamente las métricas de todos los `RtpSendStream` activos.

3.  **`Engine`:** El `Engine` se encargará de sondear a `RtpSession` para obtener las últimas métricas y emitirá nuevos eventos `EngineEvent` para notificar a la capa de la aplicación.

4.  **`RtcApp` (UI):** La aplicación de la GUI manejará los nuevos eventos de métricas y mostrará la información en la interfaz de usuario, proporcionando al usuario una visión en tiempo real de la calidad de la conexión.

## Hoja de Ruta de Implementación

### Paso 1: Procesar y Almacenar Métricas en `RtpSendStream`

El primer paso es implementar la lógica para procesar y almacenar las métricas de RTCP en `RtpSendStream`.

- **Ubicación:** `src/rtp_session/rtp_send_stream.rs`
- **Tareas:**
    1.  Añadir campos a la estructura `RtpSendStream` para almacenar las últimas métricas recibidas:
        ```rust
        pub struct RtpSendStreamMetrics {
            pub fraction_lost: u8,
            pub packets_lost: u32,
            pub highest_sequence: u32,
            pub jitter: u32,
            pub rtt: Option<Duration>,
        }
        ```
    2.  Implementar el método `on_report_block` para:
        -   Extraer la `fraction_lost`, `cumulative_packets_lost`, `extended_highest_sequence_number` y el `interarrival_jitter` del `ReportBlock`.
        -   Calcular el RTT utilizando la marca de tiempo de llegada y los campos `last_sr` y `delay_since_last_sr`.
        -   Actualizar las métricas almacenadas en `RtpSendStream`.
    3.  Añadir un método `get_metrics()` a `RtpSendStream` para devolver una instantánea de las métricas actuales.

### Paso 2: Exponer las Métricas desde `RtpSession`

Una vez que `RtpSendStream` pueda proporcionar las métricas, `RtpSession` necesita unificarlas y exponerlas.

- **Ubicación:** `src/rtp_session/rtp_session.rs`
- **Tareas:**
    1.  Crear un nuevo método en `RtpSession` llamado `poll_metrics()`.
    2.  Este método iterará sobre todos los `RtpSendStream` activos, llamará a `get_metrics()` en cada uno y devolverá una colección de métricas (por ejemplo, un `HashMap<u32, RtpSendStreamMetrics>`).

### Paso 3: Crear Nuevos Eventos `EngineEvent` para Métricas RTCP

El `Engine` debe ser capaz de comunicar las métricas a la capa de la aplicación.

- **Ubicación:** `src/core/events.rs` y `src/core/engine.rs`
- **Tareas:**
    1.  Añadir un nuevo variant al enum `EngineEvent`:
        ```rust
        pub enum EngineEvent {
            // ... otros eventos
            RtcpMetrics { ssrc: u32, metrics: RtpSendStreamMetrics },
        }
        ```
    2.  En el método `poll()` del `Engine`, llamar a `rtp_session.poll_metrics()` periódicamente (por ejemplo, cada segundo).
    3.  Por cada métrica obtenida, emitir un evento `EngineEvent::RtcpMetrics`.

### Paso 4: Mostrar las Métricas en la Interfaz de Usuario

La aplicación de la GUI necesita manejar los nuevos eventos y mostrar la información.

- **Ubicación:** `src/app/rtc_app.rs`
- **Tareas:**
    1.  En el método `poll_engine_events()` de `RtcApp`, manejar el nuevo evento `EngineEvent::RtcpMetrics`.
    2.  Almacenar las métricas recibidas en el estado de `RtcApp`.
    3.  Añadir una nueva sección en la interfaz de usuario para mostrar las métricas de cada stream, por ejemplo:
        -   SSRC del stream
        -   Fracción de paquetes perdidos (como porcentaje)
        -   Jitter (en milisegundos)
        -   RTT (en milisegundos)

### Paso 5: Añadir Pruebas

Es crucial añadir pruebas para asegurar que el cálculo y el reporte de las métricas son correctos.

- **Ubicación:** `src/rtp_session/tests/` (crear un nuevo archivo de pruebas si es necesario)
- **Tareas:**
    1.  Añadir pruebas unitarias para el cálculo de RTT en `RtpSendStream`.
    2.  Añadir pruebas de integración que simulen la recepción de paquetes `ReceiverReport` y verifiquen que `RtpSession` y el `Engine` exponen las métricas correctamente.

## Conclusión

La implementación de esta hoja de ruta proporcionará una visibilidad muy necesaria sobre la calidad de la conexión de WebRTC en `rustyrtc`. Esto no solo mejorará la experiencia del usuario al proporcionar feedback en tiempo real, sino que también sentará las bases para futuras optimizaciones, como la adaptación de la tasa de bits (bitrate) en función de las condiciones de la red.
