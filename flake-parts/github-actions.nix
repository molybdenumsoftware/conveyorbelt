{ config, lib, ... }:
{
  options.githubActions.setUpNix = lib.mkOption {
    type = lib.types.listOf (lib.types.attrsOf lib.types.unspecified);
    readOnly = true;
    default = [
      {
        name = "Install Nix";
        uses = "nixbuild/nix-quick-install-action@master";
        "with".nix_conf = ''
          keep-env-derivations = true
          keep-outputs = true
        '';
      }
      {
        name = "Set up Nix cache";
        uses = "nix-community/cache-nix-action@main";
        "with".primary-key = "nix-\${{ runner.os }}";
      }
    ];
  };
  config.perSystem =
    { pkgs, ... }:
    let
      path_ = ".github/workflows/check.yaml";
    in
    {
      files.files = [
        {
          inherit path_;
          drv = pkgs.writers.writeJSON "gh-actions-workflow-check.yaml" {
            name = "nix flake check";
            on = {
              push = { };
              workflow_call = { };
            };
            jobs.default = {
              runs-on = "ubuntu-latest";
              steps =
                [
                  { uses = "actions/checkout@main"; }
                  config.githubActions.setUpNix
                  {
                    name = "nix flake check";
                    run = "nix flake -vv --print-build-logs --accept-flake-config check";
                  }
                ]
                |> lib.flatten;
            };
          };
        }
      ];
      treefmt.settings.global.excludes = [ path_ ];
    };
}
