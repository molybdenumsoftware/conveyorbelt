{ lib, ... }:
{
  options.metadata = {
    title = lib.mkOption {
      type = lib.types.singleLineStr;
      default = "wabuisere";
    };
    description = {
      plaintext = lib.mkOption {
        type = lib.types.singleLineStr;
        default = "CLI for web development that watches source, invokes rebuild, statically serves and triggers page reload";
      };
      markdown = lib.mkOption {
        type = lib.types.singleLineStr;
        default = "CLI for web development that **wa**tches source, invokes re**bui**ld, statically **se**rves and triggers page **re**load";
      };
    };
  };
}
