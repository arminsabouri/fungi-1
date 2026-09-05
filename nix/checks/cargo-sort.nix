{
  perSystem =
    { cargoWorkspaces, pkgs, ... }:
    let
      workspace = import ./workspace.nix { inherit cargoWorkspaces pkgs; };
    in
    {
      checks = workspace.mapWorkspaces "cargo-sort" (
        workspaceName: cargoWorkspace:
        pkgs.runCommand "${workspaceName}-cargo-sort"
          {
            inherit (cargoWorkspace.commonArgs) src;
            nativeBuildInputs = [ pkgs.cargo-sort ];
          }
          ''
            cargo-sort --check --workspace "$src/${builtins.dirOf cargoWorkspace.manifestPath}"
            mkdir -p "$out"
          ''
      );
    };
}
