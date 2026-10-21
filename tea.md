# Main loop skeleton

Phases:

1. Bootstrap
2. Initialize
3. Run
4. Shut down

## Bootstrap

- logging
- args
- build command
- project root

----

- await srvdir |err
- await install signal handler |err

## Initialize (signal, http_server, fs_watcher, build)

- start http_server
- start fs_watcher
- start first_build

### Run

- start browser
- **loop select**
  - signal -> shutdown
  - started browser
  - done build -> reload browser
  - fs event -> start build

### Shutdown

- stop http server
- stop  build
- stop fs_watcher
- stop browser

- **loop select**, exit if all stopped
  - stopped http_server
  - stopped build
  - stopped watcher
  - stopped browser
