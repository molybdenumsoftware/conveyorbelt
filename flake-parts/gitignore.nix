{ lib, ... }:
{
  perSystem =
    psArgs@{ pkgs, ... }:
    {
      options.gitignore = lib.mkOption {
        type = lib.types.listOf lib.types.singleLineStr;
        default = [ ];
        apply = lib.concat [ "result" ];
      };
      config.files.file.".gitignore".text =
        psArgs.config.gitignore
        |> lib.naturalSort
        |> lib.concatLines;
    };
}
