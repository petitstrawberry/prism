# **Project Prism: High-Fidelity Audio Routing Driver**

Version: 2.0.0 (Final Architecture)  
Target: macOS (Apple Silicon / Intel)  
Language: Rust (2021 Edition)

## **1\. プロジェクト概要 (Overview)**

**Prism** は、macOS向けのオープンソース・仮想オーディオドライバ・エコシステムです。商用のLoopback等と同等の「アプリケーションごとのルーティング」を、Rustによる高性能な実装とUnix哲学に基づいた構成で実現します。

### **Core Philosophy**

1. **Separation of Concerns (分離):** 音を運ぶ「土管 (Driver)」と、制御する「頭脳 (Daemon)」を完全に分離する。  
2. **Fail-Secure (厳密性):** 未知の信号は通さない。許可されたアプリのみが音を出せる。  
3. **The Kernel Truth (正確性):** Finderの見た目だけでなく、カーネルの「責任プロセス」情報を信頼の源とする。

## **2\. システムアーキテクチャ (Architecture)**

### **コンポーネント構成**

graph TD  
    subgraph "User Space (The Brain)"  
        Config\["\~/.config/prism/config.toml"\]  
        Daemon\["prismd (Rust Daemon)"\]  
        OS\_API\[("macOS APIs\\n(LaunchServices / Responsibility)")\]  
          
        Config \--\> Daemon  
        OS\_API \-.-\>|Query PID| Daemon  
    end

    subgraph "Core Audio / HAL (The Muscle)"  
        Driver\["Prism.driver (Rust cdylib)"\]  
        RingBuf\[("Ring Buffer")\]  
    end

    subgraph "Mixing Environment"  
        DAW\["REAPER / Element"\]  
    end

    Daemon \==IPC (SetProperty 'rout')==\> Driver  
    App\_A\[Chrome\] \--PID 100--\> Driver  
    App\_B\[Slack\] \--PID 200--\> Driver  
      
    Driver \--\>|Demuxed| RingBuf  
    RingBuf \==\> DAW

### **処理フロー (The 3-Stage Routing)**

Prismは\*\*「待機室 (Parking Lot)」\*\*戦略を採用し、音漏れを防ぎつつ利便性を確保します。

1. **Stage 1: Driver (Strict Silence)**  
   * ドライバに未知のPIDが入ってくる。  
   * ルーティングテーブルにないので、**破棄 (Silence)** する。  
   * *結果: 起動直後の一瞬は無音（事故防止）。*  
2. **Stage 2: Daemon (Identification)**  
   * prismd がPIDを検知。  
   * **Step A:** Responsibility API (Kernel) で真の親プロセスを特定。  
   * **Step B:** LaunchServices で Bundle ID を特定。  
3. **Stage 3: Decision (Routing)**  
   * config.toml と照合する。  
   * **Case A (設定あり):** ルールに従い、指定チャンネルへ (例: Ch 3-4)。  
   * **Case B (設定なし):** 一般アプリとみなし、**デフォルトチャンネルへ (Ch 1-2)**。  
   * *結果: ユーザーが意図した通りに音が鳴り始める。*

## **3\. コンポーネント詳細設計**

### **A. Prism Driver (Prism.driver)**

**役割:** 高速・低遅延なオーディオ転送エンジン。

* **実装:** cdylib (Core Audio HAL Plug-in)  
* **データ構造:** ArcSwap\<Vec\<Route\>\> (Lock-free Linear Search)  
  * HashMap は使用しない。アトミックなポインタすり替えで更新する。  
* **IOロジック (DoIOOperation):**  
  1. 出力バッファをゼロクリア。  
  2. PIDがテーブルにあるか確認。  
  3. あれば Mix (Add)、なければ Continue (何もしない)。  
* **IPCインターフェース:**  
  * Custom Property: 'rout'  
  * Payload: struct { pid: i32, target\_channel: u32 }

### **B. Prism Daemon (prismd)**

**役割:** プロセス監視とポリシー管理。

* **実装:** Rust Binary (LaunchAgent常駐)  
* **グルーピング戦略:**  
  * **Primary:** Responsibility API (Private) で親子関係を解決。  
  * **Secondary:** LaunchServices (NSRunningApplication) でメタデータ取得。  
  * **Fallback:** パス逆引き (.app 探索)。  
* **動作:**  
  * NSWorkspace の通知、またはポーリングで新規PIDを監視。  
  * 識別完了後、ドライバの 'rout' プロパティを叩く。

### **C. 設定ファイル (config.toml)**

\[general\]  
\# 厳密モード: trueなら設定外のアプリはミュートする (Ch 1-2に流さない)  
strict\_mode \= false 

\# ルーティングルール  
\[\[rule\]\]  
id \= "com.spotify.client"  
target \= \[3, 4\]

\[\[rule\]\]  
id \= "com.google.Chrome"  
target \= \[5, 6\]

\# "設定外のアプリ" は自動的に Ch 1-2 (Default) にルーティングされる

## **4\. 技術スタック & 依存クレート**

| Component | Crate / Library | Purpose |
| :---- | :---- | :---- |
| **Driver** | coreaudio-sys | HAL API バインディング |
| **Driver** | arc-swap | Lock-free データ共有 |
| **Driver** | libc | メモリ操作 (memcpy/memset) |
| **Daemon** | objc2, objc2-app-kit | LaunchServices (NSWorkspace) 呼び出し |
| **Daemon** | libquarantine (link) | 責任プロセスAPI呼び出し (FFI) |
| **Daemon** | toml, serde | 設定ファイル読み込み |

## **5\. 開発フェーズ (Roadmap)**

### **Phase 1: Skeleton & Passthrough**

* 「何もしないドライバ」をビルドし、ロードさせる。  
* 入力音をそのまま Ch 1-2 に流す (BlackHole相当)。  
* **Goal:** REAPERで音が出る。

### **Phase 2: Strict Driver**

* ArcSwap テーブルの実装。  
* テーブルにないPIDをミュートするロジックの実装。  
* IPC (SetProperty) の受け口を作る。  
* **Goal:** 音が出なくなる（テーブルが空なので）。

### **Phase 3: The Brain (Daemon)**

* prismd の作成。  
* Responsibility \+ LaunchServices でアプリ名を特定するロジック。  
* ドライバへのIPC送信。  
* **Goal:** アプリを起動すると、自動的に Ch 1-2 (Default) から音が出る。

### **Phase 4: Config & Routing**

* config.toml の実装。  
* 設定に基づいたチャンネル振り分け。  
* **Goal:** Chromeだけ Ch 5-6 から出る。完成。

## **6\. 結論**

この設計は、商用製品の利便性と、エンジニアリングの堅牢性を両立したものです。

* **ドライバはバカ正直に (Dumb & Fast)**  
* **デーモンはOSの真実を知っている (Smart & Correct)**  
* **挙動は安全側に (Fail-Secure)**

これで迷うことはありません。**cargo new** しましょう。