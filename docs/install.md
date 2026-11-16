# Install Moosuri / 무수리 설치

Moosuri is a **development preview**. The installer includes the Rust server/CLI,
React dashboard and SQLite support. Rust, Node, Python and Docker are not required
to run it. The separate encrypted vault and credential companion remain optional,
separate deployments; this installer does not claim to install them.

## One command / 한 줄 설치

Run in Terminal on macOS, a Linux terminal, or a **WSL2 distribution's terminal**:

```sh
curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash
```

For a fixed language / 언어를 지정하려면:

```sh
curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash -s -- --language ko
```

Replace `ko` with `en` for English. The first question otherwise selects the
language, with the terminal locale as its default. Use **↑/↓ and Enter**, or press
a numbered choice and Enter. Use `--plain` for simple numbered menus and
`--no-browser` on a machine without a graphical browser. `q` leaves setup.

언어는 처음에 한국어·영어 중 고릅니다. **↑/↓와 Enter**, 또는 번호와 Enter로
선택합니다. 단순한 터미널은 `--plain`, 화면 없는 서버는 `--no-browser`를 사용하세요.
`sudo` 없이 일반 사용자로 실행합니다. 설치기가 root 권한을 요구하지 않습니다.

## What the wizard does / 설치 과정

1. **Access / 접속 방식:** This computer is the default. It opens a loopback-only
   `http://127.0.0.1:3210` dashboard. Choose another free port when necessary.
   Tailscale and an existing HTTPS domain are also available. Those modes need
   your own Tailscale/proxy setup; the wizard displays the next step.
2. **Administrator / 관리자:** Enter a name, email and a password of at least
   12 bytes. Email is your sign-in ID; no email is sent. Passwords are hidden.
   Leave it blank to receive a temporary password on the terminal only, then
   change it at the first sign-in. Additional accounts are created later.
3. **Model / 모델:** Connect OpenAI API, Anthropic API, OpenRouter, Ollama,
   LM Studio or another compatible server, or skip for now. API keys are hidden;
   available models are queried from your actual connection. Choose model/effort
   and perform a small billed API verification before saving the default.
   ChatGPT uses your personal official sign-in in web model settings. Claude
   subscription requires its separately configured isolated Linux runner;
   Anthropic API works on every supported platform. Credentials from other apps
   are never searched or copied. A temporary admin password must be changed before
   model configuration; setup links to the authenticated web page.
4. **Startup / 자동 시작:** Choose a user service (recommended) or foreground
   operation. macOS uses launchd; Linux/WSL2 uses systemd when available. A small
   PATH block can be appended to `.zshrc`/`.bashrc` without replacing existing
   contents. In the already-open terminal use the printed `export PATH=...` line;
   the installed command is always available at `~/.local/bin/moosuri`.
5. **Dashboard / 대시보드:** The installer checks the actual service readiness and
   embedded app before reporting it ready. Sign in, connect Google if wanted, and
   invite family/friends from the administrator settings. Model tool capability
   checks remain in the same model settings page; a text probe alone does not
   certify all agent tools.

처음에는 **이 컴퓨터에서 사용**을 고르는 것이 가장 간단합니다. 모델·Google 연결은
나중에 해도 설치와 로그인은 가능합니다. API 연결과 구독 연결은 별개이며, 개인
구독이 다른 사용자에게 공유되지 않습니다. API/로컬 모델의 기본값은 실제 확인에
성공한 경우에만 저장합니다. 모델이 연결되지 않았으면 대화 답변은 아직 생성할 수 없습니다.

## Platforms / 지원 환경

| Environment | Package | Notes |
| --- | --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` | User launchd or foreground |
| macOS Intel | `x86_64-apple-darwin` | Separate native executable |
| Linux x64 | `x86_64-unknown-linux-gnu` | glibc distribution, user systemd or foreground |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | GNU/glibc; physical ARM acceptance is tracked separately |
| Windows WSL2 | Matching Linux package | Install/run inside the Linux filesystem |

Alpine/musl, native Windows executables and WSL1 are not supported. The release
notes record actual build/runtime evidence and minimum runtime requirements;
a package name is not a claim that every distribution/device was tested.

### Windows / WSL2

If WSL2 is not installed, use Microsoft's normal installation from an administrator
PowerShell, complete any requested reboot and create your Linux user:

```powershell
wsl --install -d Ubuntu
```

Check `wsl --list --verbose`: the distribution's version must be `2`. In the Ubuntu
terminal paste the same one-line installer above. From PowerShell with a default
WSL2 distribution already configured, you can also start it with:

```powershell
wsl -- bash -lc 'curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash'
```

Keep runtime/data under your Linux home, **not `/mnt/c`**, OneDrive or another
shared/synchronized filesystem. With no user systemd session, use
`moosuri start --foreground`. If desired, follow Microsoft's
[systemd guide](https://learn.microsoft.com/windows/wsl/systemd) and restart WSL
before rerunning `moosuri setup`. The installer does not change WSL configuration
or enable system services. Closing/shutting down WSL stops its processes; a WSL
user service does not keep the Windows computer awake or start WSL at Windows boot.

WSL2가 없으면 위 Microsoft 설치 명령으로 먼저 준비하고 Ubuntu 터미널에서
설치하세요. 데이터는 Linux 홈 안에 저장합니다. Windows 부팅 자동 실행이나
WSL 종료 후 계속 실행하는 기능을 보장하지 않습니다.

## Everyday commands / 자주 쓰는 명령

```sh
moosuri status                 # Installation and readiness / 설치·실행 상태
moosuri open                   # Open dashboard / 대시보드 열기
moosuri start                  # Start configured user service / 사용자 서비스 시작
moosuri stop                   # Stop user service / 서비스 중지
moosuri start --foreground     # This terminal; Ctrl+C stops / 터미널 실행
moosuri setup --language ko    # Resume setup / 설치 이어하기
moosuri uninstall             # Remove runtime, retain data / 데이터 보존 제거
```

User services start at **user login**, not necessarily host boot. Linux servers
that need service lifetime after logout require an operator-configured lingering
user/session or their own system deployment. The installer does not invoke sudo.

If a port is occupied or a service does not start, the installer reports that it is
not ready. Try foreground mode to see its error. Linux logs:
`journalctl --user -u moosuri.service -n 50`; macOS log location is printed by
`moosuri status`. Stop the existing service before starting a foreground instance.

## Data, reruns and updates / 데이터·재실행·업데이트

Default private data: `~/Library/Application Support/moosuri` on macOS,
`~/.local/share/moosuri` on Linux/WSL2 (or an explicitly configured `MOOSURI_DATA_DIR`
/ `XDG_DATA_HOME`). The runtime uses the separate `moosuri-runtime` directory beside
that OS default. CLI launcher: `~/.local/bin/moosuri`. Files are owner-private.
Existing unsafe paths, symlinks, ACL grants or unrelated installations are refused;
the installer does not relax those permissions. Use a local, nonsynchronized home.

Reruns resume setup **after signed runtime activation**, preserving the existing
administrator and model settings. They do not reinstall over existing data, reset
passwords or silently update an older binary. If activation itself was interrupted,
retain the error and existing paths for inspection; do not delete the data directory.
A pending uninstall can be resumed using the recorded runtime's `manager uninstall
--prefix ...` before a new install.

For updates, keep using the [signed stopped-update procedure](local-installation.md).
It verifies a newer signed bundle, requires a configured encrypted backup, migrates
forward and preserves recovery state. This preview installer does not add automatic
updates. Uninstall preserves private data and any changed/unrelated files; its
small PATH block is harmless and remains in your shell profile.

## Download trust / 다운로드 검증

The short shell script is readable in [install.sh](../install.sh). It selects the
platform, downloads a fixed version's archive over HTTPS and checks a SHA-256 pinned
**inside that script before executing anything from the archive**. The Rust binary
then verifies the exact bundle's Ed25519 signature, target and embedded assets.
The initial trust is the GitHub HTTPS script and its repository publisher. A script
and checksum from the same channel are not independent authenticity proofs.
For independent trust pins or offline install, use the existing
[signed bundle installer](local-installation.md). No shell hooks from a downloaded
package, recursive remote paths, public Funnel, DNS or unrelated service changes
are introduced. No CI was enabled to publish this preview.
