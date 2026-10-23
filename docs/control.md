# Control

How onehand is driven from outside a window: the socket the running process listens on, and
what a second launch does with it. The tools an agent or an outside program may call, and the
switches that allow them, are not built yet.

## The socket

- **Who binds it:** the process holding `instance.lock` (one per config directory). Holding the
  lock proves no other process owns the socket, so the holder removes a stale file before it
  binds. A process that could not take the lock at all binds nothing.
- **Where:** beside the lock, `<config dir>/onehand.sock`. A path too long for a Unix socket
  address (more than 107 bytes) moves to `$XDG_RUNTIME_DIR/onehand-<hash>.sock`, the temp
  directory when that is unset; the hash is FNV-1a over the config directory, so every build on
  that directory finds the same name.
- **Mode `0600`:** the user's own, as the config directory beside it is.
- **What is said:** JSON-RPC 2.0, one message per line. A connection is answered and closed.
  A line that is not JSON is answered `-32700`, an unknown method `-32601`; the listener goes on.
- **Where the rule lives:** core's `instance` writes and reads every line; the app's `instance`
  only carries them, on a tokio runtime of its own, and crosses to GPUI over a channel.

## A second launch

`main` takes the lock before GPUI starts, right after `--version`. When another process holds it,
the launch sends

```json
{"jsonrpc":"2.0","id":1,"method":"onehand/open","params":{"path":"/abs/path"}}
```

with its first argument made absolute (no argument: no `path`), waits up to five seconds for the
answer, and exits.

| Answer | The running process | The second launch |
|---|---|---|
| a folder | opens it as a project in the window in front (the first one when none is), selecting it if it is one already, and raises that window | exits 0 |
| no path | raises that window | exits 0 |
| not a folder (`-32602`) | nothing | prints `onehand: <path> is not a folder`, exits 1 |
| nobody on the socket | (an older build, or one that could not bind) | prints `onehand is already running on <dir>`, exits 1 |
