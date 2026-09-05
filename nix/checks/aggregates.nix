{
  perSystem =
    { config, pkgs, ... }:
    let
      registry = config.workspaceChecks;
      entries =
        workspaceName: checks:
        pkgs.lib.mapAttrsToList (name: check: {
          name = "${workspaceName}-${name}";
          path = check.package;
        }) checks;
      tagged = tag: pkgs.lib.filterAttrs (_: check: builtins.elem tag check.tags);
      selectNamed =
        checkName:
        pkgs.lib.concatLists (
          pkgs.lib.mapAttrsToList (
            workspaceName: checks:
            entries workspaceName (pkgs.lib.filterAttrs (name: _: name == checkName) checks)
          ) registry
        );
      selectTagged =
        tag:
        pkgs.lib.concatLists (
          pkgs.lib.mapAttrsToList (workspaceName: checks: entries workspaceName (tagged tag checks)) registry
        );
      join = name: paths: pkgs.linkFarm name paths;
    in
    {
      checks = {
        tests = join "tests" (selectNamed "tests-nightly-dev");
        clippy = join "clippy" (selectNamed "clippy");
        coverage = join "coverage" (selectNamed "coverage");
        quick = join "quick" (selectTagged "quick");
        lint = join "lint" (
          [
            {
              name = "no-todo-comments";
              path = config.checks.no-todo-comments;
            }
          ]
          ++ selectTagged "lint"
        );
        nightly = join "nightly" (selectTagged "nightly");
      }
      // pkgs.lib.concatMapAttrs (workspaceName: checks: {
        ${workspaceName} = join workspaceName (entries workspaceName checks);
        "${workspaceName}-quick" = join "${workspaceName}-quick" (
          entries workspaceName (tagged "quick" checks)
        );
      }) registry;
    };
}
