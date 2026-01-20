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

              > [!CAUTION]
              > This software is not yet ready for use

              ## Usage

              ```
              $ ${config.metadata.title} <build command>
              ```

              A temporary directory *serve path* is created
              and its contents statically served at `http://localhost:<available port>/`.
              A chromium browser is launched with that URL.

              On file changes the `<build command>` is invoked
              with the *serve path* provided as the environment variable `${psArgs.config.buildEnv.SERVE_PATH}`.
              When `<build command>` exits successfully, the page reloads.
            ''
            |> pkgs.writeText "README.md";
        }
      ];
    };

  # TODO document difference between this design and watching the serve path
  # TODO document different between this design and JS injection for reloading
}
