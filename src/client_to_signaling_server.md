# Implementation Requirement

1. **Error signal vs. connection state**

   * You send `SignalingEvent::Error(String)` and then later `Disconnected`.
   * `send()` returns `SignalingClientError::Frame(FrameError::Io(_))` when the write fails, but `connected` stays `true` until the reader notices and flips it.

   This is fine, but be aware: `is_connected()` is “has the reader noticed a failure yet”, not “did the last write succeed”. In practice that’s OK, since a failed `send` is immediately surfaced to the UI.

2. **Poisoned mutex**

   `Poisoned` is a reasonable error, but in a GUI app it usually indicates “we panicked in this thread earlier”. You might just treat `Poisoned` as “Disconnected” and nuke the client.

---

## 2. RTC app + signaling integration

#### 2.1. Hang-up semantics are incomplete

On the **client**:

* When *you* hang up (`CallFlow::Active` + “Hang up” button), you call:

  ```rust
  fn reset_call_flow(&mut self) {
      self.call_flow = CallFlow::Idle;
      self.pending_remote_sdp = None;
      self.engine.stop();
  }
  ```

  That **does not send any signaling message**. The remote side has no idea you hung up. It will remain in `CallFlow::Active`.

* When the **remote** hangs up:

  * The server (as currently written) never forwards a `Msg::Bye` to anybody.
  * What it does emit is `Msg::PeerLeft { .. }` to remaining session members.
  * Your `RtcApp::handle_server_msg` doesn’t handle `PeerLeft` at all; it will fall into the `other` arm and only log “Unhandled signaling message: PeerLeft {…}”.

So currently:

* Local hang-up = local stop only.
* Remote hang-up (via `Bye`) = remote sees nothing meaningful, unless you join sessions and watch `PeerLeft`.


**keep things simple, ignore Sessions for now**

Extend your protocol so `Bye` is a peer-to-peer signal:

```rust
Bye { from: UserName, to: UserName, reason: Option<String> }
```

* Server: treat `Bye` like `Offer/Answer/Candidate` and forward it in `forward_signaling`.
* Client:

  * When user clicks “Hang up”, send `Msg::Bye { .. }` then `reset_call_flow()`.
  * When receiving `Msg::Bye { .. }`, show “Peer ended call…” and call `reset_call_flow()`.

# Instructions
- Drop Sessions and Make call teardown symmetric: We only want 1:1 calls for this project, simplify the server: make it a pure forwarder for `Offer/Answer/Candidate/Bye/Ack`, and push Sessions into “group calls later”.

- **Add basic keep-alive / liveness**
   * Use `Ping`/`Pong` from the client side (a periodic ping in the network thread).
   * If you don’t see a `Pong` or any message for N seconds, treat as disconnected.


# Current Progress

• Hooked the GUI into the signaling system:

  - Extended the signaling protocol/server to cover registration success responses,
    peer-list queries (ListPeers/PeersOnline), and embedded sender metadata in Offer/
    Answer/Candidate. Relaxed the forwarding guard so logged-in peers can exchange
    SDPs without a shared session and added coverage tests confirming peer-list
    handling and registration responses.
  - Added a reusable signaling_client module that opens a TCP connection, performs the
    handshake, spawns a background reader thread, and surfaces a simple SignalingEvent
    stream plus a send API for client messages.
  - Reworked app/rtc_app.rs to manage signaling UI state (connect/login/home screens), track the current user and online peers, and drive call flow. Users can now connect, register or log in, view peers, initiate calls (sending offers via the server), and accept incoming offers to auto-generate answers. Incoming server
    messages update UI state and kick off SDP application.
  - Wired incoming ICE Candidate messages from the server into the engine so the GUI
    applies trickled remote candidates instead of only logging them.
  - Fixed the home peer list borrow issue that was breaking compilation and cleaned up
    the warnings reported by cargo check (unused imports/vars, locking mutability).
  - Emit ACKs for received Offers/Answers and send local ICE candidates over signaling
    when sending Offers/Answers so peers get trickled host candidates automatically.
  - Fixed the signaling server peer-list behavior for unauthenticated clients (returns
    empty list as expected by tests) and improved logging visibility.
  - Default user store now lives next to the signaling server binary (unless
    `RUSTYRTC_USERS_PATH` is set), so persisted users survive server restarts even if
    the working directory changes. Incoming Offer/Answer bodies now populate the UI
    remote SDP field for visibility.
  - Made call teardown symmetric: `Bye` now carries from/to and is forwarded by the
    server, the UI sends it on hang-up/decline, and remote Bye tears down the call
    state. Ack now includes from/to and is forwarded peer-to-peer.
  - Added client heartbeat: signaling client pings periodically, marks the connection
    as disconnected on heartbeat timeout, and logs all signaling traffic for easier
    debugging. Poisoned locks now drop the connection instead of bubbling poison.

# cargo check
Now compiles
# cargo test
Ok! All tests pass.
# Manual test
Works, but when hanging up, the other client doesn't notices.
