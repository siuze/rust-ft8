# rust-ft8: 纯 Rust 实现的 FT8 物理层编解码与解调库

[![Crates.io](https://img.shields.io/crates/v/rust-ft8.svg)](https://crates.io/crates/rust-ft8)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

`rust-ft8` 是一个采用纯 Rust 实现的业余无线电 FT8 协议编解码与解调库。该项目不包含 C/Fortran 外部动态库依赖，支持在 Linux (x86_64/ARM64)、Windows、macOS 以及嵌入式 Linux 设备（如 RK3588、RK3568 等）上编译运行。

---

## 目录
1. [项目概述](#一项目概述)
2. [技术实现与模块设计](#二技术实现与模块设计)
3. [多平台测试数据与对比](#三多平台测试数据与对比)
4. [已知缺陷与优化计划](#四已知缺陷与优化计划)
5. [解码配置与多线程控制](#五解码配置与多线程控制)
6. [快速开始与代码示例](#六快速开始与代码示例)
7. [命令行工具使用](#七命令行工具使用)
8. [参考项目与开源协议](#八参考项目与开源协议)
9. [人工智能辅助开发与免责声明](#九人工智能辅助开发与免责声明)

---

## 一、项目概述

本项目依据公开的技术文献（见 `docs/FT4_FT8_QEX.pdf`）以及业余无线电社区已有的开源实现编写，提供 FT8 信号的调制、同步检测、解调滤波、LDPC 译码、多轮信号相干消减（Signal Subtraction）以及 77-bit 报文解析功能。

---

## 二、技术实现与模块设计

- **纯 Rust 实现**：消除 C/Fortran 交叉编译链的复杂配置与 FFI 运行时开销；
- **平台向量化指令加速**：
  - x86_64：利用 AVX2/FMA/SSE 进行滑动 FFT 与矩阵点积并行计算；
  - ARM64：利用 NEON 向量指令集优化基带抽取滤波计算；
- **流式分段解码机制**：
  - 支持在音频接收过程中持续喂入分片数据；
  - 在 $t = 11.36\text{s}$（时隙剩余 3.64s，数据符号接收完毕）时，将未到达的尾导码置为擦除（Erasure），进行提前译码；
  - 在 $t = 12.48\text{s}$（发射结束，时隙剩余 2.52s）时完成全时隙扫尾并发出完结回调；
- **时域信号消减 (Signal Subtraction)**：
  - 重构已译码信号的时域复包络，从原始音频缓冲区中相干扣除，降低强信号旁瓣对同频段弱信号的干扰；
- **HASH 码与 77-bit 报文解析**：
  - 兼容 12-bit / 22-bit 呼号哈希的打包与还原，哈希值统一格式化输出为大写十六进制；
- **DXCC 实体查询与网格计算**：
  - 采用 256 桶表索引与并发只读缓存加速呼号前缀查找；
  - 支持 4 位与 6 位梅登黑德网格互转、经纬度与大圆距离计算。

---

## 三、多平台测试数据与对比

### 1. 硬件平台执行耗时实测 (测试音频 `websdr_test1.wav`, 15.00s 录音)

| 性能指标 | PC (Intel i5-8265U) | RK3588 (8核 A76+A55) | RK3568 (4核 A55) |
| :--- | :---: | :---: | :---: |
| **指令加速方式** | AVX2 + FMA | NEON + Cortex-A76 | NEON + Cortex-A55 |
| **离线单轮极速解调耗时** | 0.28s (检出 15 条) | 0.28s (检出 15 条) | 0.84s (检出 15 条) |
| **离线 2 轮消减总耗时** | 0.50s (检出 19 条) | 0.47s (检出 19 条) | 1.26s (检出 19 条) |
| **流式前导码初筛时刻** | $t = 1.44\text{s}$ | $t = 1.44\text{s}$ | $t = 1.44\text{s}$ |
| **流式首批强信号输出时刻** | $t = 11.36\text{s}$ (提前 3.64s) | $t = 11.36\text{s}$ (提前 3.64s) | $t = 11.36\text{s}$ (提前 3.64s) |
| **首批强信号计算开销** | 0.12s | 0.07s | 0.15s |
| **全时隙扫尾完结时刻** | $t = 12.48\text{s}$ (提前 2.52s) | $t = 12.48\text{s}$ (提前 2.52s) | $t = 12.48\text{s}$ (提前 2.52s) |

---

### 2. ft8_lib 测试集 14 样本录音对比

以下测试样本均取自 `ft8_lib` 仓库自带的测试音频集（存放在本仓库 `tests/wav/` 目录下）。对比基准对象为 C 语言库 `ft8_lib` 实测值以及采用 `wsjtx-lib`（C++/Fortran 实现）解出的结果：

| 测试音频样本 (来自 ft8_lib 测试集) | ft8_lib (C库实测) | wsjtx-lib (基准实测) | rust_ft8 (实测) | RK3588 耗时 | RK3568 耗时 |
| :--- | :---: | :---: | :---: | :---: | :---: |
| `191111_110115.wav` | 0 | 1 | 4 | 0.97s | 0.99s |
| `191111_110130.wav` | 4 | 5 | 6 | 0.69s | 1.15s |
| `191111_110145.wav` | 2 | 2 | 2 | 0.52s | 0.77s |
| `191111_110200.wav` | 4 | 5 | 9 | 0.76s | 1.15s |
| `191111_110215.wav` | 3 | 5 | 5 | 0.98s | 1.12s |
| `191111_110615.wav` | 17 | 22 | 18 | 1.23s | 2.22s |
| `191111_110630.wav` | 12 | 19 | 14 | 1.25s | 1.69s |
| `191111_110645.wav` | 16 | 19 | 22 | 1.19s | 2.03s |
| `191111_110700.wav` | 14 | 18 | 18 | 1.15s | 1.81s |
| `websdr_test1.wav` | 13 | 19 | 20 | 1.31s | 1.92s |
| `websdr_test2.wav` | 19 | 23 | 23 | 1.50s | 2.52s |
| `websdr_test3.wav` | 9 | 16 | 12 | 1.65s | 1.71s |
| `websdr_test4.wav` | 21 | 27 | 24 | 1.39s | 2.35s |
| `websdr_test5.wav` | 17 | 28 | 26 | 1.34s | 2.28s |
| **总计 (14 组样本)** | **151 条** | **209 条** | **203 条** | **16.00s** | **23.74s** |

---

## 四、已知缺陷与优化计划

在 14 组样本的比对中，总计条数看似相近（wsjtx-lib 209 条，rust_ft8 203 条），但二者检出的信号并非完全一致。实际存在明显的并集与交集差异：

1. **存在漏检消息**：在多个密集录音样本中，`rust_ft8` 相比 `wsjtx-lib` 存在未能检出的信号（例如在 `191111_110615` 漏检 4 条，在 `191111_110630` 漏检 5 条，在 `websdr_test3` 漏检 4 条等，全量 14 组样本中累计约有 20 余条在 wsjtx-lib 中成功检出而在本库中漏检）；
2. **总计条数被拉平的原因**：在另一些底噪较低或多信号重叠的录音中（如 `191111_110115`、`191111_110200`、`191111_110645`），`rust_ft8` 依靠相干信号消减解出了一些 wsjtx-lib 未能检出的微弱信号，从而在表面总数上填补了漏检的差额。

### 缺陷根因分析
- **OSD 回溯深度与剪枝策略**：为保证嵌入式平台的计算速度，当前实现的 OSD 译码器限制了迭代次数与候选翻转位数，导致部分处于门限边缘的弱相关码字被提前截断；
- **频漂（Drift Rate）跟踪缺失**：当前相干消除算法假设信号在 12.64 秒内载波恒定无漂移，遇到存在频偏漂移的信号时，相干消除残差偏大；
- **部分非标准报文格式覆盖不全**：少数罕见的特种比赛与遥测格式（Type 3/4 变体）尚未全部覆盖。

### 后续优化计划与进展
- [x] 增加可选的自适应深搜译码模式（`DecoderConfig.deep_search = true`），放开 OSD 满秩搜索深度与多符号相干通道，补齐边缘漏检信号；
- [x] 引入频漂（Drift Rate $\Delta f / \Delta t$）联合估计与动态调频时域相干消除，提升密集弱信号消减深度；
- [x] 完善 77-bit 罕见特种报文格式（Type 0.1~0.4, Type 3, Type 5）解析覆盖；
- [ ] 借鉴 WSJT-X 2.6/2.7 的 A Priori (AP) 先验信息译码机制，对已知通联呼号注入先验 LLR（后续放最后阶段实现）。

---

## 五、解码配置与多线程控制

### 1. 解码配置结构体 (`DecoderConfig`)

```rust
pub struct DecoderConfig {
    /// 搜索通带下限频率 (Hz)，默认 100.0 (规避工频 50/60Hz 杂散可调整为 200.0)
    pub nfa: f32,
    /// 搜索通带上限频率 (Hz)，默认 3500.0
    pub nfb: f32,
    /// 信号相干消减迭代轮数:
    /// 1: 单轮极速解调 (PC 0.28s, RK3568 0.84s)
    /// 2: 推荐默认 (耗时 0.47~1.26s，解出绝大部分重叠信号)
    /// 3: 深度挖掘 (-24dB 极限弱信号)
    pub passes: usize,
    /// Costas (7x7) 同步检测归一化门限，默认 1.4 (有效范围 1.2~1.8)
    pub sync_min: f32,
    /// 深度搜索模式：放开 OSD 回溯深度与全相干通道，大幅提升微弱信号检出率 (默认 false)
    pub deep_search: bool,
    /// 是否启用频漂跟踪 (Drift Rate) 与动态调频波形消减 (默认 true)
    pub enable_drift: bool,
}
```

### 2. Rayon 多线程控制

`rust-ft8` 使用 `rayon` 进行多候选信号的并行特征提取与译码：
- **环境变量控制（推荐）**：
  ```bash
  export RAYON_NUM_THREADS=4  # 在 4 核嵌入式设备上限制并发线程数
  ```
- **全局线程池初始化**：
  ```rust
  rayon::ThreadPoolBuilder::new().num_threads(4).build_global().ok();
  ```
- **独立线程池调用**：
  ```rust
  let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
  let signals = pool.install(|| decode_audio(&samples, &config, -0.9));
  ```

---

## 六、快速开始与代码示例

> 完整 API 接入指南与函数参考请参阅 [docs/API_GUIDE.md](docs/API_GUIDE.md)。

在 `Cargo.toml` 中添加依赖：
```toml
[dependencies]
rust-ft8 = "0.1.0"
```

### 1. 音频文件离线解码

```rust
use rust_ft8::{decode_audio, DecoderConfig, read_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (audio, _sr) = read_wav_file("tests/wav/websdr_test1.wav")?;
    let config = DecoderConfig::default();

    // 传入 window_start_offset = -0.9 (针对提前 0.9s 开始录音的 DT 校正)
    let messages = decode_audio(&audio, &config, -0.9);
    for m in messages {
        println!(
            "{:4.0} Hz | SNR:{:+3} dB | DT:{:+5.2}s | {} ({}) -> {} | 网格: {}",
            m.freq, m.snr, m.dt, m.sender_callsign, m.country_cn, m.receiver_callsign, m.grid
        );
    }
    Ok(())
}
```

### 2. 文本消息编码与 WAV 合成

```rust
use rust_ft8::{encode_message_to_audio, encode_message_to_tones, write_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = "CQ BD4SUR OM99";

    // 1. 生成 79 个音调符号序列 (0..=7)
    let tones = encode_message_to_tones(message)?;

    // 2. 合成 12000Hz PCM 音频 (载波 1000Hz, 延迟 0.5s, 补齐 15s 窗口)
    let audio = encode_message_to_audio(message, 1000.0, 12000, 0.5, true)?;
    write_wav_file("cq.wav", &audio, 12000)?;
    Ok(())
}
```

---

## 七、命令行工具使用

编译构建工具：
```bash
cargo build --release --bins
```

- **`ft8_decode`**：离线音频文件解码工具
  ```bash
  ./target/release/ft8_decode tests/wav/websdr_test1.wav --passes 2 --nfa 200 --nfb 3000
  ```
- **`ft8_stream`**：流式解码回放模拟工具
  ```bash
  ./target/release/ft8_stream tests/wav/websdr_test1.wav
  ```
- **`ft8_encode`**：文本消息音频合成工具
  ```bash
  ./target/release/ft8_encode "CQ BD4SUR OM99" tx.wav --freq 1500.0
  ```

---

## 八、参考项目与开源协议

### 1. 参考项目及区别说明

本项目在算法设计、数据结构定义以及测试验证过程中参考了以下开源项目：

| 项目名称 | 源码仓库地址 | 主要作者 / 组织 | 开源许可证 | 说明与关系 |
| :--- | :--- | :--- | :---: | :--- |
| **WSJT-X** | [github.com/WSJTX/wsjtx/releases](https://github.com/WSJTX/wsjtx/releases) | Joe Taylor (K1JT), Steve Franke (K9AN) 等 | **GPL-3.0** | FT8 协议的原创项目与官方 Fortran 参考实现。本项目的协议规范与 77-bit 报文定义均以此为基准。 |
| **wsjtx-lib** | [github.com/paulh002/wsjtx_lib](https://github.com/paulh002/wsjtx_lib) | 业余无线电开源社区 | **GPL-3.0** | 早期将 WSJT-X 算法核心剥离出的独立 C++ 包装库（本项目测试集使用的 `cli_decode.exe` 基于此构建）。注意：该独立库未集成 WSJT-X 2.6/2.7 引入的动态 AP 先验译码逻辑。 |
| **ft8_lib** | [github.com/kg4sgp/ft8_lib](https://github.com/kg4sgp/ft8_lib) | Karlis Goba (YL3JG) | **MIT** | 微控制器轻量 C 语言库。本项目测试集中的 14 组 WAV 样本及参考消息直接引用自该项目的测试用例。 |
| **JTDX Improved** | [sourceforge.net/projects/jtdx-improved](https://sourceforge.net/projects/jtdx-improved/) | JTDX 社区 / Igor Chernikov 等 | **GPL-3.0** | 衍生版本，其多步滤波与微弱信号处理逻辑为本项目提供了算法设计参考。 |
| **JS8Call** | [github.com/jjs/js8call](https://github.com/jjs/js8call) | Jordan Sherer (KN4CRD) | **GPL-3.0** | 基于 FT8 调制的文本通信软件，其网状分包与心跳应答设计为本项目的扩展通信设计提供了参考。 |

### 2. 开源协议说明 (License Statement)

本项目在实现过程中参考了采用 GPL-3.0 许可的 `wsjtx-lib` 与 `WSJT-X` 的校验矩阵定义、Costas 同步和时域信号消减流程。依据 GNU 通用公共许可证的要求，**本项目采用 GNU General Public License v3.0 (GPL-3.0-or-later) 协议开源**。

完整许可证条款请参阅项目根目录下的 [LICENSE](LICENSE) 文件。

---

## 九、人工智能辅助开发与免责声明

本项目在算法梳理、代码编写、平台适配及测试用例构建过程中，使用了人工智能编程代理（AI Agent）辅助完成。

由于物理层数字信号处理与信道编译码算法具有高度复杂性，尽管代码经过了多平台基准测试与交叉验证，仍可能存在潜在的算法逻辑缺陷、数值精度损失或边界条件未覆盖等情况。

请使用者在将本库用于实际无线电硬件、外场通联或二次开发时，自行评估相关风险，谨慎使用代码，并在关键场景下进行充分的独立验证与测试。
