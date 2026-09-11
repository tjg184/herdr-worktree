# Changelog

## [1.2.0](https://github.com/tjg184/herdr-worktree/compare/v1.1.2...v1.2.0) (2026-09-11)


### Features

* **tui:** show new worktree action immediately while branch list loads ([6790111](https://github.com/tjg184/herdr-worktree/commit/6790111a1387c0fd8a9c023c801cdf4126ebdd6a))


### Bug Fixes

* **remove:** focus previous workspace after unloading worktree ([75c4bc8](https://github.com/tjg184/herdr-worktree/commit/75c4bc89c256367bc9627ab5ea58ee3cd8c04db8))
* **remove:** focus return workspace before closing to avoid process termination ([b988cb1](https://github.com/tjg184/herdr-worktree/commit/b988cb19834b33d9bc61fb957a9acc2b56ed8b9e))
* **remove:** return to main checkout workspace using repo_key match ([b0009ca](https://github.com/tjg184/herdr-worktree/commit/b0009ca8439c3afd70766327bf2c1c3fd734dc4e))


### Performance Improvements

* **tui:** load branch list in background to eliminate startup blank screen ([86510ac](https://github.com/tjg184/herdr-worktree/commit/86510ac7d641d1852650c7c57a7d40d6230c4506))

## [1.1.2](https://github.com/tjg184/herdr-worktree/compare/v1.1.1...v1.1.2) (2026-08-13)


### Bug Fixes

* load remotes async ([0504cbb](https://github.com/tjg184/herdr-worktree/commit/0504cbb49d38003f197f5c5cd9f5a8c28a21a773))
* remove messages and able to just close workspace ([ca84f74](https://github.com/tjg184/herdr-worktree/commit/ca84f7452164a6b191cce69c861be9b49b45c45b))

## [1.1.1](https://github.com/tjg184/herdr-worktree/compare/v1.1.0...v1.1.1) (2026-08-10)


### Bug Fixes

* change remote key to control+r ([a1d9db7](https://github.com/tjg184/herdr-worktree/commit/a1d9db7191edfd2d978f2bdffc0ae415a3123d4c))
* lower herdr requirement ([d7400a4](https://github.com/tjg184/herdr-worktree/commit/d7400a43b1cce4e6e2663fcb56b31b7073ded5e8))

## [1.1.0](https://github.com/tjg184/herdr-worktree/compare/v1.0.0...v1.1.0) (2026-08-09)


### Features

* **backend:** add native herdr worktree backend ([b9748c1](https://github.com/tjg184/herdr-worktree/commit/b9748c1a4f4393f080eaef368cf873287c1c00bc))

## [1.0.0](https://github.com/tjg184/herdr-worktree/compare/v0.1.0...v1.0.0) (2026-08-09)


### Features

* add guided worktree creation and remote refresh ([1e935ec](https://github.com/tjg184/herdr-worktree/commit/1e935ec0c12368885c0f635abafa435826ea1c99))
* add release process ([cf21ba1](https://github.com/tjg184/herdr-worktree/commit/cf21ba1ee87c99be659737bebdbc6fad210f9866))
* **ui:** improve new worktree chooser presentation ([59e3cd8](https://github.com/tjg184/herdr-worktree/commit/59e3cd8cd245070ddb54ba3030a5ee68f74aef27))


### Bug Fixes

* don't create branch when selecting remote worktree ([3420f71](https://github.com/tjg184/herdr-worktree/commit/3420f71c354157a4201b2cf78fec9d1db6ee321a))
* **picker:** preserve branch identity on selection ([508557f](https://github.com/tjg184/herdr-worktree/commit/508557f057e862516fec21c02db42061946381f0))


### Miscellaneous Chores

* release 1.0.0 ([f8731d0](https://github.com/tjg184/herdr-worktree/commit/f8731d01b97749a343d13205b9a3fada250f998e))
