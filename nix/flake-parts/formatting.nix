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
        rustfmt.enable = true;
        alejandra.enable = true;
        prettier.enable = true;
      };
      settings.on-unmatched = "fatal";
    };
  };
}
