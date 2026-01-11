{ inputs, ... }:
let
  spdx = "MIT";
in
{
  flake-file.inputs.license = {
    url = "https://spdx.org/licenses/${spdx}.txt";
    flake = false;
  };

  cargoManifest.package.license = spdx;

  perSystem =
    let
      path_ = "LICENSE";
    in
    {
      treefmt.projectRootFile = path_;
      files.files = [
        {
          inherit path_;
          drv = inputs.license;
        }
      ];

      treefmt.settings.global.excludes = [ path_ ];
    };
}
