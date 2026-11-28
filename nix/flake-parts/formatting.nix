{inputs, ...}: {
  flake-file = {
    formatter = pkgs: pkgs.alejandra;
    inputs.treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      flake = false;
    };
  };

  imports = ["${inputs.treefmt-nix}/flake-module.nix"];

  perSystem = {
    treefmt = {
      programs = {
        taplo = {
          enable = true;
          settings.formatting = {
            reorder_keys = true;
            reorder_arrays = true;
            reorder_inline_tables = true;
            allowed_blank_lines = 0;
          };
        };
        rustfmt.enable = true;
        alejandra.enable = true;
        prettier.enable = true;
        nixf-diagnose.enable = true;
      };
      settings.on-unmatched = "fatal";
    };
  };
}
