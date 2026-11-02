# conda 频道发布

频道地址:

```text
https://mixsoulgithub.github.io/conda-channel
```

包内容就是 GitHub Release 的 `x86_64-unknown-linux-musl` 静态二进制;频道本身是
`mixsoulgithub/conda-channel` 的 GitHub Pages,不是 anaconda.org。只发布 `linux-64`。

## 一次性准备

用独立的 micromamba 环境,不要污染日常环境:

```fish
micromamba create -y -p ~/.local/share/conda-release -c conda-forge conda-build conda-index
```

## 每版步骤

1. 改 `packaging/conda/meta.yaml` 的 `version` 和对应 musl 包的 `sha256`。
   重打包同一版本时把 `build: number` 加一。
2. 构建:

```fish
rm -rf /tmp/conda-output
~/.local/share/conda-release/bin/conda-build packaging/conda --output-folder /tmp/conda-output
~/.local/share/conda-release/bin/conda-build --test /tmp/conda-output/linux-64/graph-tui-<version>-<build>.conda
```

3. 把 `.conda` 放进频道并重建索引:

```fish
set ch /tmp/conda-channel
test -d $ch; or gh repo clone mixsoulgithub/conda-channel $ch
cp /tmp/conda-output/linux-64/graph-tui-<version>-<build>.conda $ch/linux-64/
~/.local/share/conda-release/bin/conda index $ch
```

`conda index` 要在频道根目录跑,不要只指到 `linux-64`,否则不会更新 `linux-64/repodata.json`。

4. 提交并推送:

```fish
git -C $ch add README.md linux-64
git -C $ch commit -m "graph-tui <version> (linux-64, static musl)"
git -C $ch push origin main
```

5. 等 GitHub Pages 构建完成:

```fish
gh api repos/mixsoulgithub/conda-channel/pages/builds --jq '.[0] | {status,commit}'
```

6. 用干净环境从远端频道安装验证:

```fish
micromamba create -y -p /tmp/gtui-conda-test -c https://mixsoulgithub.github.io/conda-channel -c conda-forge graph-tui
/tmp/gtui-conda-test/bin/graph-tui --version
```

## 备注

- 只支持 `linux-64`;recipe 里的 `skip` 会拒绝 mac/arm,避免把 musl 二进制装错平台。
- `source` 指向 GitHub Release;GitHub 的 release-assets 偶尔很慢,conda-build 会缓存到
  `~/.local/share/conda-release/conda-bld/src_cache/`。下载失败时先把资产用 `wget -c`
  拉到缓存文件名再重试,不要改 recipe 的 URL/sha。
- 频道仓库保留 `index.html`、`repodata.json`、`.cache/cache.db`;这些由 `conda index` 更新,
  不要手工编辑。
- micromamba 会缓存 repodata;刚发完频道如果搜不到,先 `micromamba clean -i -y` 再搜。
