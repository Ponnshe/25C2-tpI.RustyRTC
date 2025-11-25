# Arquitectura de Procesamiento de Medios (RTP → Depacketizer → Decoder)
Visión General

El sistema implementa una canalización reactiva basada en eventos y canales internos, en la cual cada componente es un actor especializado. Ningún módulo comparte estado directo. Toda comunicación sucede por eventos dirigidos y canales unidireccionales.

La responsabilidad de gestionar transporte, jitter, timestamps, PTs y demás metadatos RTP está completamente encapsulada en RtpSession.
El procesamiento de medios se realiza únicamente mediante:

- RtpSession (@src/rtp_session/rtp_session.rs) recibe y envia paquetes RTP, este tiene receivers y senders workers (thread para evitar para el programa)
- MediaTransport (@src/media_transport/media_transport.rs) maneja la comunicación entre RtpSession y MediaAgent. Este tiene depacketizers y packetizers workers (threads para evitar para el programa)
- MediaAgent (@src/media_agent/media_agent.rs) maneja el codec. Este debe tene coders y decoders workers (threads para evitar parar el programa)

El flujo de un paquete rtp recibido es el siguiente:
1. Se recibe en RtpSession por un recv_stream, este notifica a engine por un evento que llego un paquete RTP.
2. Engine (@src/core/engine.rs) pasa este evento a MediaTransport el cual debe pasarselo al depacketizer worker
3. Cuando el depacketizer worker termine entonces le notificará a MediaTransport (no a Engine, una vez llega un paquete Engine no debe de saber nada mas de este)
4. MediaTransport se lo comunicará a MediaAgent
5. MediaAgent se lo pasara al decoder worker

El flujo al enviar un paquete:
1. MediaAgent tiene un frame 
2. MediaAgent se lo notifica a un encoder
3. El encoder notifica cuando termina a MediaAgente
4. MediaAgent se lo notifica a MediaTransport
5. MediaTransport se lo notifica a un packetizer 
6. El packetizer al terminar notifica a MediaTransport
7. MediaTransport le notifica a RtpSession
8. RtpSession se lo notifica a un sender

Responsabilidades de Cada Componente
1. `RtpSession`

- Único responsable de toda lógica RTP/RTCP:

    - Manejo de SSRCs, PTs, RTCP, streams de entrada/salida.

    - Mapear paquetes entrantes a RtpRecvStream.

    - Emitir eventos de recepción RTP hacia el motor de eventos (Engine).

- El único punto de entrada de paquetes UDP.

- El único módulo que toca jitter, timestamps, RTCP, pérdida de paquetes, etc.

Output relevante:
Cuando recibe un RTP válido, genera un evento:
EngineEvent::IncomingRtpPacket { ssrc, payload_type, data }

2. Engine

Único punto de distribución de eventos globales.

Cuando detecta un evento IncomingRtpPacket, se lo entrega exclusivamente a MediaTransport.

No interactúa con depacketizer ni decoder. Solo routes.

3. MediaTransport

Responsabilidad central: gestionar el flujo entre RtpSession → Depacketizer → MediaAgent.

Funciones clave:

a) Recepción desde Engine

Recibe eventos del estilo:

IncomingRtpPacket { ssrc, payload_type, data }

b) Selección del Depacketizer

Mapea payload_type → Depacketizer correspondiente.
(Aquí vive el conocimiento de PTs. MediaAgent jamás los ve.)

c) Orquestación del Procesamiento

Cuando recibe un paquete:

Envía el paquete crudo al depacketizer.

Espera a que el depacketizer emita un evento interno indicando que terminó de procesar:

Ejemplo: DepacketizerEvent::ChunkReady { codec, chunk }

Ese evento NO se reenvía al Engine, solo a MediaTransport.

MediaTransport convierte el resultado en un evento para MediaAgent:

MediaTransportEvent::DecodedChunkReady { codec, chunk }

4. Depacketizer

Componente autónomo.

Recibe paquetes RTP del MediaTransport.

Reconstruye unidades de datos a nivel de códec (NALUs, Opus frames, etc.).

Al terminar, emite un evento solo para MediaTransport, nunca para Engine.

No conoce decoders, solo genera unidades de carga procesables.

5. MediaAgent

Puente final entre la capa de transporte y los decodificadores.

Responsabilidades:

Recibe eventos internos de MediaTransport del tipo:

DecodedChunkReady { codec, chunk }

Selecciona el decoder adecuado en función del codec, nunca del payload_type.

Envía el chunk al decoder correspondiente.

No conoce ningún dato de RTP.

6. Decoder

Entidad autónoma que solo sabe decodificar chunks del codec asignado.

Recibe datos únicamente desde MediaAgent.

Emite su salida final (frames, samples, etc.) mediante sus propios métodos o eventos internos.

# Flujo Completo de un Paquete RTP

Llega paquete UDP a RtpSession.

RtpSession identifica si es RTP o RTCP.

Si es RTP válido, genera EngineEvent::IncomingRtpPacket.

Engine recibe el evento → lo pasa solo a MediaTransport.

MediaTransport:

Usa PT para elegir depacketizer.

Envía paquete al depacketizer.

Depacketizer reconstruye unidad → genera DepacketizerEvent::ChunkReady.
(Evento privado, solo para MediaTransport).

MediaTransport recibe el evento → genera MediaTransportEvent::DecodedChunkReady.

MediaAgent recibe el evento → elige decoder por codec.

Decoder decodifica el chunk.

# Eventos Necesarios (No exhaustivo)

Cada módulo puede definir sus propios eventos y canales, según necesidad. Lo importante es que:

Engine solo maneja eventos globales.

MediaTransport solo usa eventos internos propios y del depacketizer.

MediaAgent solo usa eventos internos de MediaTransport.

Decoder/Depacketizer manejan sus propios listeners internos.

Ejemplos conceptuales (no código real):

Eventos del Engine → MediaTransport

IncomingRtpPacket

Eventos Depacketizer → MediaTransport

ChunkReady

Eventos MediaTransport → MediaAgent

DecodedChunkReady

Eventos internos de MediaAgent → Decoder

DecodeChunk

# Restricciones Técnicas

No se permite el uso de crates externos para comunicación o concurrencia.
Debe usarse únicamente:

std::sync::mpsc

std::sync::{Arc, Mutex}

std::thread

std::sync::atomic

La arquitectura debe respetar completamente:

No acoplar depacketizer con decoder.

No exponer PTs a MediaAgent.

No exponer códecs a MediaTransport excepto para mapearlos.
