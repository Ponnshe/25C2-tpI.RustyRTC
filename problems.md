# Problemas con `RtcApp`, `Engine` y manejo de estados de la aplicación
Actualmente, apenas se crea `MediaTransport`, se construyen los (de)packetizer, el `MediaAgent`, su decoder y encoder, además del camera worker. Todo este mecanismo empieza a trabajar (encodear frames locales e intentarlos enviar) cuando la negociación aún ni siquiera ha sucedido. Esto se puede verificar en los logs de la última versión de `packetizer_refactor`. 

Un extracto de los logs después de hacer `cargo run`:
```
[Debug] 1763494859184 | [MediaAgent] queued local frame (ts=1763494859180, force_keyframe=true)
[Info] 1763494859219 | [MediaAgent] Camera error: Failed to open camera with device_id: 0. Using test pattern.
[Debug] 1763494859228 | [MediaAgent] queued local frame (ts=1763494859223, force_keyframe=false)
[Debug] 1763494859249 | [MediaAgent] encoded frame ready for transport (ts=1763494859180)
[Debug] 1763494859270 | [MediaAgent] queued local frame (ts=1763494859265, force_keyframe=false)
[Debug] 1763494859284 | [MediaAgent] encoded frame ready for transport (ts=1763494859223)
[Debug] 1763494859310 | [MediaAgent] queued local frame (ts=1763494859308, force_keyframe=false)

```

Evidentemente, esto no está bien. Para solucionar esto, considero que MediaTransport debería mantener un estado booleano interior `run_flag` que se propague mínimo al thread `media-agent-camera-worker`, wrappeándolo con un `Arc<AtomicBool>`, de forma que el camera worker puede estar listo desde el comienzo de la aplicación, pero solo empiece a obtener frames de la cámara cuando `Session` termine de hacer el handshake y pase el estado de la aplicación a `ConnState::Running`.

Además, noto que el estado en `RtcApp` se está manejando pobremente. Se hace `engine.snapshot_frames`  sin requerir que la conexión esté establecida (`ConnState::Running`), es decir, no hay validación contra el estado de la conexión de la aplicación. 

Otro problema es que se habilita el botón `Start Connection` inmediatamente después de tener ambos SDPs, cuando aún existe la posibilidad de que todavía no se haya nominado un par ICE, dando lugar a bugs en donde presionar el botón inmediatamente después de ser habilitado puede generar comportamiento imprevisto. Este botón solo se debería habilitar solo si un par ICE ya fue nominado, y solo recién al presionarse debería empezar el handshake. Con las herramientas que poseemos actualmente, esta situación es complicada de resolver dado que no hay una variante de `ConnState` que refleje este estado de la conexión (en dónde ya se hizo la nominación ICE pero el cliente todavía no decide iniciar la conexión (el botón `Start Connection` todavía no hay sido clickeado). Tal vez, lo más pertinente sería añadir una variante nueva `ConnSate::IceNominated`. El cuál, justamente refleje un estado intermedio entre `Idle` y `Running` y sirva para para disparar el inicio del handshake, sin todavía hacer trigger de las acciones que se toman con `ConnState::Running`, como podría ser iniciar el inicio del envio de frames locales.

Por el punto previamente comentado,  considero que se debería implementar un método en `Engine` (por ejemplo: `start_media_sending`), el cual sea llamado  dentro de `rtc_app.rs` después de que el estado de `rtc_app` pase a ser `ConnState::Running`. De esta manera, el método podría propagar este cambio de estado hacia `MediaAgent` y este último hacia `media-agent-camera-worker`, dando lugar al inicio de obtención de frames locales y encodeo de ellos.
