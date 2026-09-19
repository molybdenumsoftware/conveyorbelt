{ inputs, ... }:
let
  spdx = "MIT";
in
{
  flake-file.inputs.license = {
    url = "https://spdx.org/licenses/${spdx}.txt";
    flake = false;
  };

  workspaceManifest.workspace.package.license = spdx;

  perSystem =
    let
      path_ = "LICENSE";
    in
    {
      treefmt.projectRootFile = path_;
      files.file.${path_}.source = inputs.license;

      treefmt.settings.global.excludes = [ path_ ];
    };
}
