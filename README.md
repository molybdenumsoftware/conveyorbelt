# conveyorbelt

CLI for web development that watches source, invokes rebuild, statically serves and triggers page reload

> [!CAUTION]
> This software is not yet ready for use

## Usage

```
$ conveyorbelt <build command>
```

A temporary directory *serve path* is created
and its contents statically served at `http://localhost:<available port>/`.
A chromium browser is launched with that URL.

On file changes the `<build command>` is invoked
with the *serve path* provided as the environment variable `SERVE_PATH`.
When `<build command>` exits successfully, the page reloads.
