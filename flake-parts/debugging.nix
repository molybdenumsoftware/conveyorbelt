{
  config.perSystem = psArgs: {
    drv = {
      env = {
        LOG_FILTER_VAR_NAME = "LOG";
        LOG = "trace";
      };
      buildType = "debug";
      dontStrip = true;
      checkFlags = [ "--nocapture" ];
    };
  };
}
