{
  perSystem = {pkgs, ...}: let
    template = ''
      # ${pkgs.conveyorbelt.crateName}

      ${pkgs.conveyorbelt.meta.description}

      > [!CAUTION]
      > This software is not yet ready for use

      ## Usage

      ```
      @cli_help@
      ```

      ## Prior art

      - [tapio/live-server](https://github.com/tapio/live-server)
      - [lomirus/live-server](https://github.com/lomirus/live-server)

      Both suffer the same design problem; they watch the same directory that they serve.
      That results in the browser being instructed to reload on each file change,
      without regard to whether a build process had completed or merely made its first change.
      That design also lacks the convenience of automatic build command invocation.

      Another difference is that ${pkgs.conveyorbelt.crateName} controls the browser
      using [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
      while the projects mentioned above use a WebSocket and JavaScript that is injected into the served page.
    '';
  in {
    files.file."README.md".source =
      pkgs.runCommand "README.md" {
        src = pkgs.writers.writeText "README.template.md" template;
        nativeBuildInputs = [pkgs.conveyorbelt];
      } ''
        set -euo pipefail
        cli_help=$(conveyorbelt --help)
        substitute "$src" "$out" --replace-fail "@cli_help@" "$cli_help"
      '';
  };
}
