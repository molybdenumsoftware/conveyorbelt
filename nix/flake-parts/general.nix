{
  lib,
  rootPath,
  inputs,
  ...
}: let
  meta = import (rootPath + "/nix/meta.nix") lib;
in {
  debug = true;

  flake-file = {
    description = "This flake is for development purposes only. Please use the overlay.";

    nixConfig = {
      abort-on-warn = true;
      extra-experimental-features = ["pipe-operators"];
    };

    inputs = {
      flake-file.url = "github:denful/flake-file";
      flake-parts.url = "github:hercules-ci/flake-parts";
      import-tree = {
        url = "github:vic/import-tree";
        flake = false;
      };
    };
  };

  imports = [inputs.flake-file.flakeModules.default];

  systems = meta.platforms;

  perSystem = {pkgs, ...}: {
    checks.package = pkgs.conveyorbelt;
  };
}
