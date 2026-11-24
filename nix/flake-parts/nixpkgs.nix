{
  inputs,
  rootPath,
  ...
}: {
  flake-file.inputs.nixpkgs = {
    url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    flake = false;
  };

  perSystem = {
    system,
    pkgs,
    ...
  }: {
    imports = ["${inputs.nixpkgs}/nixos/modules/misc/nixpkgs.nix"];
    nixpkgs.hostPlatform = {inherit system;};
    legacyPackages = pkgs;
    nixpkgs.overlays = [(import (rootPath + "/nix/overlay.nix"))];
  };
}
