# conveyorbelt

Web dev workflow; stack-agnostic, hand-coded 🦀

> [!CAUTION]
> This software is not yet ready for use

## Usage

```
A CLI daemon that can:

- watch source
- invoke arbitrary build command
- statically serve
- launch browser
- trigger page reload
- politely report what it's doing

A temporary directory *serve path* is created
and its contents statically served at `http://localhost:<available port>/`.
A chromium browser is launched with that URL.

Project files, as determined by git, are watched for changes.
On file changes the `<build command>` is invoked.
The *build process* receives the *serve path* via the environment variable `SERVE_PATH`.
When the *build process* exits successfully, the page reloads.


Usage: conveyorbelt <BUILD_COMMAND>

Arguments:
  <BUILD_COMMAND>
          The build command

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## Prior art

- [tapio/live-server](https://github.com/tapio/live-server)
- [lomirus/live-server](https://github.com/lomirus/live-server)

Both suffer the same design problem; they watch the same directory that they serve.
That results in the browser being instructed to reload on each file change,
without regard to whether a build process had completed or merely made its first change.
That design also lacks the convenience of automatic build command invocation.

Another difference is that conveyorbelt controls the browser
using [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
while the projects mentioned above use a WebSocket and JavaScript that is injected into the served page.
