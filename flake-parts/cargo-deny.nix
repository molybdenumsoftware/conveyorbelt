{
  config,
  lib,
  toSource,
  rootPath,
  ...
}:
{
  binManifest.bin = [
    {
      name = config.metadata.title;
      path = "src/main.rs";
    }
  ];

  perSystem =
    { craneLib, pkgs, ... }:
    let
      path_ = "deny.toml";
    in
    {
      files.files = [
        {
          inherit path_;
          drv = pkgs.writers.writeTOML "deny.toml" {
            licenses.allow = [
              "Apache-2.0"
              "CC0-1.0"
              "ISC"
              "MIT"
              "Unicode-3.0"
              "Zlib"
            ];
          };
        }
      ];

      treefmt.settings.global.excludes = [ path_ ];

      checks.cargo-deny = craneLib.cargoDeny {
        src =
          [
            config.filesets.workspaceManifest
            config.filesets.binManifest
            config.filesets.lockFile
            (rootPath + "/${path_}")
          ]
          |> lib.fileset.unions
          |> toSource;
      };
    };
}
