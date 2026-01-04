# conveyorbelt

CLI for web development that watches source, invokes rebuild, statically serves and triggers page reload

## Usage

```
$ conveyorbelt <build command>
```

A *serve path* will be resolved to `<git top-level>/serve`
and its contents statically served at `http://localhost:<available port>/`.
`chromium` will be launched with that URL.

git tracked files will be watched.
On change, the `<build command>` will be invoked with *serve path* provided via the environment as `SERVE_PATH`.
When `<build command>` exits successfully, the page reloads.
