# DO-NOT-EDIT. This file was auto-generated using github:denful/flake-file.
# Use `nix run .#write-flake` to regenerate it.
{
  description = "This flake is for development purposes only. Please use the overlay.";

  outputs = inputs: import ./outputs.nix inputs;

  nixConfig = {
    abort-on-warn = true;
    extra-experimental-features = ["pipe-operators"];
  };

  inputs = {
    files = {
      url = "github:mightyiam/files";
      flake = false;
    };
    flake-file.url = "github:denful/flake-file";
    flake-parts.url = "github:hercules-ci/flake-parts";
    import-tree = {
      url = "github:vic/import-tree";
      flake = false;
    };
    license = {
      url = "https://spdx.org/licenses/MIT.txt";
      flake = false;
    };
    make-shell = {
      url = "github:nicknovitski/make-shell";
      flake = false;
    };
    nixpkgs = {
      url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
      flake = false;
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      flake = false;
    };
  };
}
