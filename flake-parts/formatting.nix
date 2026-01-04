{ inputs, ... }:
{
  flake-file.inputs.treefmt-nix = {
    url = "github:numtide/treefmt-nix";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem = {
    treefmt = {
      programs = {
        rustfmt.enable = true;
        nixfmt.enable = true;
        taplo = {
          enable = true;
          settings.formatting = {
            reorder_keys = true;
            reorder_arrays = true;
            reorder_inline_tables = true;
            allowed_blank_lines = 1;
          };
        };
      };
      settings.on-unmatched = "fatal";
    };
  };
}
