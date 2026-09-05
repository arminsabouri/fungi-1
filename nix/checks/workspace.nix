{ cargoWorkspaces, pkgs }:
{
  checkArgs =
    workspace:
    workspace.commonArgs
    // {
      dontFixup = true;
      doInstallCargoArtifacts = false;
      CARGO_PROFILE = "";
    };

  mapWorkspaces =
    checkName: mkCheck:
    pkgs.lib.mapAttrs' (
      workspaceName: workspace:
      pkgs.lib.nameValuePair "${workspaceName}-${checkName}" (mkCheck workspaceName workspace)
    ) cargoWorkspaces;
}
