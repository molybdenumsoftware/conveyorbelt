{ config, ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      path_ = "README.md";
    in
    {
      treefmt.settings.global.excludes = [ path_ ];
      files.files = [
        {
          inherit path_;
          drv =
            ''

            ''
            |> pkgs.writeText "README.md";
        }
      ];
    };
}
