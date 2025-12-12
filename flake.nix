# DO-NOT-EDIT. This file was auto-generated using github:vic/flake-file.
# Use `nix run .#write-flake` to regenerate it.
{
  description = "CLI for web development that watches source, invokes rebuild, statically serves and triggers page reload";

  outputs = inputs: import ./outputs.nix inputs;

  nixConfig = {
    abort-on-warn = true;
    extra-experimental-features = [ "pipe-operators" ];
  };

  inputs = {
    files.url = "github:mightyiam/files";
    flake-compat = {
      flake = false;
      url = "https://git.lix.systems/lix-project/flake-compat/archive/main.tar.gz";
    };
    flake-file.url = "github:vic/flake-file";
    flake-parts = {
      inputs.nixpkgs-lib.follows = "nixpkgs";
      url = "github:hercules-ci/flake-parts";
    };
    import-tree.url = "github:vic/import-tree";
    make-shell = {
      inputs.flake-compat.follows = "";
      url = "github:nicknovitski/make-shell";
    };
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    systems.url = "github:nix-systems/default";
    treefmt-nix = {
      inputs.nixpkgs.follows = "nixpkgs";
      url = "github:numtide/treefmt-nix";
    };
  };

}
