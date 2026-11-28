{
  conveyorbeltWorkspace,
  writableTmpDirAsHomeHook,
}:
conveyorbeltWorkspace.workspaceMembers.conveyorbelt.build.override {
  runTests = true;
  testInputs = [writableTmpDirAsHomeHook];
}
