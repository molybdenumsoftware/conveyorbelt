{
  rootPath,
  lib,
  inputs,
  ...
}: let
  meta = import (rootPath + "/nix/meta.nix") lib;
in {
  flake-file.inputs.license = {
    url = "https://spdx.org/licenses/${meta.license.spdxId}.txt";
    flake = false;
  };

  perSystem = {
    files.file.LICENSE.text = builtins.readFile inputs.license;
  };
}
