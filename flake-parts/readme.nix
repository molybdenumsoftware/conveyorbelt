{ config, ... }:
{
  perSystem =
    psArgs@{ pkgs, ... }:
    let
      path_ = "README.md";
    in
    {
      treefmt.settings.global.excludes = [ path_ ];
      files.files = [
        {
          inherit path_;
          drv =
            ''
              # ${config.metadata.title}

              ${config.metadata.description}

              ## Usage

              ```
              $ ${config.metadata.title} <build command>
              ```

              A *serve path* will be resolved to `<git top-level>/${psArgs.config.drv.env.SERVE_DIR}`
              and its contents statically served at `http://localhost:<available port>/`.
              `chromium` will be launched with that URL.

              git tracked files will be watched.
              On change, the `<build command>` will be invoked with *serve path* provided via the environment as `SERVE_PATH`.
              When `<build command>` exits successfully, the page reloads.
            ''
            |> pkgs.writeText "README.md";
        }
      ];
    };
}
