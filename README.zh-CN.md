# HJson

[English](README.md)

HJson 是一个用 Rust、GPUI 和 gpui-kit 构建的原生 JSON 查看与结构对比工具。它适合快速打开来自文件、剪贴板、手动输入、URL 或会话历史的 JSON，并以格式化文本和可导航结构树的方式检查内容。

所有解析、格式化、对比和历史状态都保留在本机应用内。

## 功能

- 从剪贴板、文件、URL、手动输入、示例数据或会话历史打开 JSON。
- 以格式化文本、结构树，或左右并排的方式查看文档。
- 在文档工具栏中切换三种展示模式：左右分栏、仅文本、仅结构树。
- 按 key 或 value 过滤结构树，展开/收起节点，并把树节点选择联动到原文行。
- 从结构树右键菜单复制 key、标量值、子树、JSONPath / JSON Pointer / jq 路径，或完整文档。
- 在新标签页打开任意树节点子树，也支持把可解析的 JSON 字符串打开为独立文档。
- 对 `created_time`、`updated_at`、`epoch`、`timestamp` 等时间语义字段展示时间戳提示。
- 按 JSONPath 对比两份 JSON：对象 key 顺序无关，数组按下标对齐。
- 在设置中调整语言、外观、JSON 字体、自动换行、行号、key 排序和语法主题。
- 通过 tag release 产出 macOS、Linux 和 Windows 安装包或压缩包。

## 快捷键

非 macOS 平台使用 `Ctrl` 替代下表中的 `Cmd`。

| 快捷键 | 操作 |
| --- | --- |
| `Cmd+T` | 新建导入标签页 |
| `Cmd+D` | 新建对比标签页 |
| `Cmd+O` | 打开文件 |
| `Cmd+Shift+V` | 从剪贴板粘贴 JSON |
| `Cmd+W` | 关闭标签页 |
| `Cmd+Shift+H` | 切换历史侧栏 |
| `Cmd+1` ... `Cmd+9` | 选择标签页 |
| `Cmd+F` | 聚焦文档结构树过滤框 |
| `Cmd+Shift+E` | 展开结构树 |
| `Cmd+Shift+C` | 收起结构树 |
| `Cmd+[` / `Cmd+]` | 在差异间跳转 |
| `Cmd+=` / `Cmd+-` / `Cmd+0` | 放大、缩小、重置缩放 |
| `Cmd+Ctrl+F` | macOS 切换全屏 |
| `F11` | 非 macOS 平台切换全屏 |

在文档页内，`Tab` 会在文本区和结构树之间切换焦点。结构树中可以用方向键和 `Enter` 导航、展开节点。原文文本区中可以用方向键、Page、Home 和 End 滚动。

## 开发

前置条件：

- Rust stable 工具链。
- GPUI 所需的平台原生构建依赖。

运行应用：

```sh
cargo run
```

检查项目：

```sh
cargo fmt --all -- --check
cargo check
cargo test
git diff --check
```

构建 release 二进制：

```sh
cargo build --release
```

## 发布

推送 `v*` tag 会触发 GitHub Actions release workflow，并发布以下平台的安装包或压缩包：

- macOS arm64 和 x86_64
- Linux arm64 和 x86_64
- Windows arm64 和 x86_64

手动运行 workflow 只会构建并上传 artifacts，不会发布 GitHub Release。
