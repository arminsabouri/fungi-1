{
  perSystem =
    {
      cargoWorkspaces,
      config,
      pkgs,
      toolchains,
      ...
    }:
    let
      profiles = [
        "dev"
        "release"
      ];
      workspaceNames = builtins.attrNames cargoWorkspaces;
      testNames = pkgs.lib.concatMap (
        toolchainName: map (profile: "tests-${toolchainName}-${profile}") profiles
      ) (builtins.attrNames toolchains);
      detailedNames = [
        "build"
        "cargo-shear"
        "cargo-sort"
        "clippy"
        "coverage"
        "doc"
        "unused-lints"
      ]
      ++ testNames;
      select =
        checkName:
        map (workspaceName: {
          name = "${workspaceName}-${checkName}";
          path = builtins.getAttr "${workspaceName}-${checkName}" config.checks;
        }) workspaceNames;
      selectMany = checkNames: builtins.concatMap select checkNames;
      join = name: entries: pkgs.linkFarm name entries;
    in
    {
      checks = {
        tests = join "tests" (select "tests-nightly-dev");
        clippy = join "clippy" (select "clippy");
        coverage = join "coverage" (select "coverage");
        quick = join "quick" (selectMany [
          "tests-nightly-dev"
          "clippy"
        ]);
        lint = join "lint" (
          [
            {
              name = "no-todo-comments";
              path = config.checks.no-todo-comments;
            }
          ]
          ++ selectMany [
            "cargo-shear"
            "cargo-sort"
            "clippy"
            "doc"
            "unused-lints"
          ]
        );
        nightly = join "nightly" (selectMany detailedNames);
      }
      // pkgs.lib.mapAttrs' (
        workspaceName: _:
        pkgs.lib.nameValuePair workspaceName (
          join workspaceName (
            map (checkName: {
              name = "${workspaceName}-${checkName}";
              path = builtins.getAttr "${workspaceName}-${checkName}" config.checks;
            }) (detailedNames ++ [ "mutants" ])
          )
        )
      ) cargoWorkspaces;
    };
}
