# lab-engine 작업 인계 (2026-09-25)

다음 세션이 이 문서만 읽고 이어서 작업할 수 있도록 정리했다. 설계 원칙과 로드맵 전체는 `DESIGN.md`에 있다.

## 사용자 지시 (우선)

- 1차 목표는 **PokaiEngine급 엔진**이다(arXiv 2608.29197, Max Yu). 이 엔진은 행동 조합 하나의 결과 분포 전체를 한 번에 열거하고, Showdown과 약 99% 일치한다고 보고됐다. 코드는 공개되지 않았다.
- 대응 범위는 **포켓몬 챔피언스뿐**이다. 규칙과 메커니즘은 Champions를 따른다.
- **기믹 (2026-09-25 사용자 명확화):** Champions M-C 규칙셋은 메가진화만 허용하고 나머지 활성화 방식은 거부한다. 울트라버스트·Z기술·다이맥스(거다이맥스는 다이맥스 상태로 표현)·테라스탈은 **꺼 둔 것이지 제거한 것이 아니다.** 행동·상태 구조에 남겨 두고 규칙셋으로 켤 수 있게 유지한다. 원시회귀는 선택하는 기믹이 아니고 자동 폼체인지로 다룬다.
- **규칙에서 벗어나지 않으면 레귤레이션에 없는 포켓몬·기술·도구도 쓸 수 있다.** 엔진에 합법성 목록을 넣지 않는다.
- 형식은 **더블(M-C)을 먼저** 맞춘다. 싱글은 `State<1>`로 컴파일되는 구조만 유지한다.
- 기존 골격(다른 세션 "Stockfish 포켓몬 AI"가 커밋 `e38207f`로 만든 `engine/core`)을 이어서 쓴다.

## 현재 상태

| 항목 | 상태 |
|---|---|
| `core/` Rust 골격 | `State<N>`, 되돌릴 수 있는 `Instruction`(apply/reverse), 필드·진영 효과 테이블, `SlotAction`, `Evaluator`. **턴 진행과 기술 효과 연결은 아직 없다.** |
| `core/` 능력치·데미지 원시 연산 | `stats.rs`에 Champions SP 제한/능력치 공식, `damage.rs`에 4096 고정소수점 보정·16개 정수 데미지 롤. Rillaboom의 Grassy Glide → Tyranitar 오라클 롤 `[146..174]`과 정확 일치. 아직 기술·특성·도구 훅 및 턴 진행에는 연결하지 않았다. |
| `core/` 기믹·규칙셋 | `gimmick.rs`(`Gimmick` 6종, 1바이트 `GimmickSet`, `DynamaxState`), `rules.rs`(`Ruleset::CHAMPIONS_MC` = 메가만, 슬롯·진영 행동 검증 `ActionError`, 기믹을 붙인 합법 행동 생성 `joint_actions`), `Side.gimmicks_used`, 되돌릴 수 있는 `Instruction::UseGimmick`. 기믹의 실제 효과는 미구현. 테스트 11개 통과(GNU, 2026-09-25) |
| `oracle/enumerate.cjs` | Showdown 정답 분포 추출기. `full` / `extremes` / `mc` 모드. 동작 확인함 |
| `oracle/canonical.cjs` | 엔진과 oracle이 공유하는 정규 상태 JSON(schema 1). 포켓몬은 이름으로 식별 |
| `oracle/compare.cjs` | 두 분포의 TV 거리와 차이가 큰 결과를 출력. MC가 섞이면 잡음 기준으로 종료 코드를 정한다 |
| `oracle/scenarios/` | `hypnosis-gravity`, `single-hit`, `spread-damage`, 공용 팀 `hypnosis-gravity.p1/p2.json` |
| `data/export.cjs` → `data/champions.json` | Champions 모드 dex 전체(종 1518, 기술 938, 도구 583, 특성 321, 타입, 성격, 주요 상태). `isNonstandard`는 태그로만 남기고, 콜백으로 구현된 동작은 `handlers`에 이름만 남긴다(= 직접 구현할 목록) |
| `data/gen-rust.cjs` → `core/src/dex/generated.rs` | **Rust 정적 테이블(2026-09-25).** 종 1517(MissingNo. 제외: Bird 타입이 상성표에 없음), 기술 938, 도구 583, 특성 321, 타입 19+상성표·타입 면역, 성격 25, 이름으로 참조되는 조건 110. 모르는 필드·값 형태·이름 참조가 나오면 생성이 실패한다. 생성 파일은 직접 고치지 않는다 |
| `core/src/dex/mod.rs` | 테이블 타입과 조회 API. `SpeciesId`·`MoveId`·`ItemId`·`AbilityId`·`ConditionId`(인덱스 0 = 없음), `from_id`(이진 탐색)·`from_name`, 항목별 상수(`species::GARDEVOIR_MEGA`, `moves::HYPNOSIS`, `conditions::GRAVITY`). `Pokemon`의 종·도구·특성·타입·기술 필드도 이 ID 타입으로 바꿨다. 테스트 21개 통과(GNU), fmt·clippy 통과 |
| `scenario/` (`lab-scenario`) | **시나리오·팀 JSON → `State<2>` 로더(2026-09-25, 검증 완료).** serde/serde_json은 이 크레이트에만 있고 `lab-engine`은 의존성 없음 그대로다. 종·기술·도구·특성·성격은 dex API로 찾고 모르는 이름은 편·팀 위치·이름을 붙인 오류로 거부한다. 능력치는 `stats::champions_stats`(SP 검증 포함). 레벨·타입·HP·5능력치·상태·도구·특성·기술 4칸·PP를 채운다. 파티 순서 = 팀 프리뷰 순서(Showdown `chooseTeam`처럼 순서 문자열을 팀 크기로 자르고 빠진 멤버는 원래 순서로 뒤에 붙임), 앞 N마리가 선두. 표시 이름·팀 위치·성격·SP·성별·테라 타입은 `State` 밖 사이드카(`ScenarioMeta`/`SideMeta`)에 둔다. `setupTurns`·`patch`(비어 있지 않을 때)·custom game 외 형식·레벨 50 외·모르는 JSON 필드는 거부한다 |
| `core/` 로더 지원 | `state::champions_max_pp`/`MoveSlot::full`(Champions PP: `(pp/5+1)*4`, PP 증가 불가 기술은 기본값), `gimmick::mega_evolution`/`structural_gimmicks`(메가스톤의 `mega_stone` 표에서 정확한 종 일치로 메가 자격만 도출). 다른 기믹 자격은 도출하지 않고, 규칙셋도 M-C에서 막는다 |
| `scenario/src/switch_in.rs` | **첫 등장 펼치기(2026-09-25, 검증 완료).** `initial_outcomes(&LoadedScenario)`/`expand_switch_ins(&State<N>)` → `Vec<InitialOutcome { probability, state }>`. Showdown `runSwitch` 순서(저장 S 내림차순, 동속 균등 분기), 트레이스 균등 대상 분기(`NOTRACE`), 복사 특성 즉시 시작, 모래날림·그래스메이커(5턴, 보송보송바위/그라운드코트 8턴). 구현 목록 밖의 시작 핸들러는 오류로 거부. `State`와 `lab-engine`은 바꾸지 않았다 |
| `scenario/src/canonical.rs` | **정규 상태 schema 1 출력(2026-09-25, 검증 완료).** `canonical_json`(canonicalKey와 같은 바이트열)·`canonical_value`. 표현 못 하는 상태는 `CanonicalError::Unrepresentable` |
| `oracle/initial.cjs` → `oracle/expected/single-hit.initial.json` | 초기 분포 oracle(팀 프리뷰→첫 결정 열거 + 고정 시드 `before`). Showdown 고정 커밋에서 재생성 완료: 2분기·2결과, 각 1/2 |
| `DESIGN.md` | 범위, oracle, 데이터, 빌드, 로더, 등장 펼치기·정규 출력, 로드맵 갱신 |

**커밋하지 않았다.** `engine/` 아래 변경 전부(`core/src/dex/`, `scenario/`, `oracle/initial.cjs`, `oracle/expected/` 포함)와 `.github/workflows/engine.yml`(생성 파일 최신 여부 검사, `lab-scenario` clippy·test 추가)이 미커밋 상태다. 사용자 승인 없이 커밋하지 않는다.

**로더 검증(2026-09-25):** serde 의존성과 `Cargo.lock`을 갱신했고, GNU에서 `lab-engine` 28개 + `lab-scenario` fixture 6개 테스트가 통과했다. rustfmt, GNU Clippy(`-D warnings`), dex 생성 최신 검사도 통과했다.

**등장 펼치기·정규 출력 검증(2026-09-25):** `oracle/initial.cjs`를 실제 Showdown에서 실행해 fixture를 재생성했다. Rust의 두 초기 결과가 oracle과 상태·확률 모두 정확히 일치하고, 고정 시드 `before`가 그중 하나와 일치한다. `lab-engine` 28개 + 기존 loader fixture 6개 + 초기/canonical 9개 테스트가 통과했다. `speed_ties_branch_and_smooth_rock_extends_sand`의 기대 분포(1/4·1/4·1/2)와 `canMega` 출력은 Showdown 코드를 읽고 유도했으며 별도 oracle 시나리오는 아직 없다.

## 검증 결과 (2026-09-25)

- `hypnosis-gravity`(중력 아래 최면술, 속이기는 방어에 막힘): 결과 2가지, 수면 2턴 2/3, 3턴 1/3. 명목 대상 난수를 접으면 분기가 768개에서 6개로 줄고 결과는 같다.
- `single-hit`(그래스슬라이더 1회): `full` 2112분기 → 결과 271가지, 7.3초. `mc` 4000회와 비교하면 TV 0.088이고 잡음 추정치는 약 0.10이라 일치한다.
- `spread-damage`(광역기 2개 + 단일기 2개): `extremes` 모드도 10분을 넘겨 중단했다. `mc` 3000회는 8.3초, 결과 167가지. 이런 턴은 `mc`로 통계 검사한다.
- Showdown 재실행 비용은 분기당 약 3.5ms다.
- `damage_rolls` 릴리스 마이크로벤치(20,000,000회 × 5): 중앙값 25.40ns/16롤 분포, 초당 약 3,938만 분포. 이는 저수준 데미지 원시 연산 수치이며 전체 턴 열거 속도는 아니다.

## 실행 방법

```bash
node engine/oracle/enumerate.cjs engine/oracle/scenarios/single-hit.json --out "$TEMP/full.json"
node engine/oracle/enumerate.cjs engine/oracle/scenarios/single-hit.json --mode mc --samples 4000 --out "$TEMP/mc.json"
node engine/oracle/compare.cjs "$TEMP/full.json" "$TEMP/mc.json"
node engine/data/export.cjs
node engine/data/gen-rust.cjs            # champions.json → core/src/dex/generated.rs (--check: CI용 최신 여부 검사)
# 로컬 빌드 (2026-09-25 이후: 기본 MSVC 툴체인, 산출물은 D:\cargo-target)
cd engine && cargo fmt --all --check
cd engine && cargo clippy --workspace --all-targets -- -D warnings
cd engine && cargo test --workspace --exclude lab-engine-py     # py 크레이트는 extension-module이라 cargo test 불가(CI도 제외)
cd engine && cargo bench -p lab-engine --bench damage
cd engine/py && ../../.venv-doubles/Scripts/maturin.exe build --release -i ../../.venv-doubles/Scripts/python.exe --out D:/cargo-target/wheels
../.venv-doubles/Scripts/python.exe -m pip install --force-reinstall --no-deps D:/cargo-target/wheels/lab_engine-0.1.0-cp312-abi3-win_amd64.whl
```

- **로컬 빌드 체계 (2026-09-25 사용자 지시로 전환. 이전 "로컬 빌드 금지" 방침은 폐기.)**
  - **MSVC(기본):** VS Build Tools 2022 17.14 C++ 워크로드를 `D:\VS\BuildTools`에 설치(MSVC 14.44.35207, Windows SDK 26100). vswhere가 찾으므로 rustc가 `link.exe`를 자동 인식한다. 설치 스크립트·로그: `D:\VS\install-buildtools.ps1`, `D:\VS\install-buildtools.log`. 실제 폴더가 삭제된 채 남아 있던 옛 인스턴스 메타데이터(Build Tools 2022, VS 2019 Community)는 `D:\VS\stale-instances-backup`으로 옮겨 두었다. rustup override는 없으므로 기본 MSVC 툴체인이 쓰이고, CI Windows 잡과 같다. fmt·clippy·test·maturin 모두 MSVC에서 통과 확인. py 크레이트 빌드 시 `linker stdout: ... .lib ... .exp 개체를 생성` 경고는 MSVC 링커 안내 메시지(`linker_messages`)로 무해하다.
  - **GNU(대안):** `cargo +stable-x86_64-pc-windows-gnu ...`. GNU 툴체인에 rustfmt·clippy 설치됨. rustup 자체 포함 MinGW에는 `as.exe`가 없어 pyo3(raw-dylib) 링크 시 dlltool이 실패하므로, 전역 설정이 GNU 타깃 링커·dlltool을 기존에 있던 `D:\winlibs`(GCC 16.2.0 UCRT)로 잡는다. GNU에서도 전 과정 통과 확인.
  - `%USERPROFILE%\.cargo\config.toml`(전역, 저장소 밖): `target-dir = D:/cargo-target` 및 위 GNU 링커 설정.
  - **CUDA 13.4.1** 툴킷을 `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.4`에 설치(2026-09-25, winget `Nvidia.CUDA`, 약 3.2 GB, `CUDA_PATH` 설정됨). **디스플레이 드라이버는 제외**했다(드라이버 591.86 유지, 지원 CUDA 13.1이지만 13.x 마이너 버전 호환으로 동작). 설치 구성 요소: nvcc, crt, cudart, nvvm, nvptxcompiler, cupti, nvrtc(+dev), nvjitlink, nvfatbin, nvml_dev, nvtx, thrust(cccl), cublas(+dev), curand(+dev), sanitizer, visual_studio_integration, cuda_profiler_api, cuobjdump, nvprune, cuxxfilt, nvdisasm, occupancy_calculator. cufft·cusolver·cusparse·npp·nvjpeg·nsight는 미설치이며 필요 시 `winget install --id Nvidia.CUDA --force --override "-s -n <구성요소_13.4 ...>"`로 추가한다. 첫 설치 때 `crt_13.4`·`nvvm_13.4`·`nvptxcompiler_13.4`를 빠뜨려 `crt/host_config.h` 없음 오류가 났고 추가 설치로 해결했다. 구성 요소 이름은 NVIDIA Windows 설치 가이드 표 참고.
  - 검증: 벡터 덧셈 커널(`nvcc -ccbin <D:\VS\BuildTools ...\cl.exe> -arch=sm_75`)이 GTX 1660 SUPER(sm_75)에서 1,048,576/1,048,576 정답. MSVC 헤더 코드페이지 경고 C4819는 무해(`-Xcompiler /utf-8`로 억제 가능). GPU 실험은 `wgpu`(Vulkan)로도 가능. 장비: Ryzen 5 3500X 6코어, 32 GB, GTX 1660 SUPER 6 GB.
  - `.venv-doubles`에 pip(ensurepip)·maturin 1.15.0 설치. wheel `lab_engine-0.1.0-cp312-abi3-win_amd64.whl` 빌드·설치·`slots("doubles")==2` 확인.
  - `py/src/lib.rs`는 `::lab_engine::`로 참조한다. `#[pymodule] fn lab_engine`이 같은 이름의 모듈을 만들어 의존 크레이트를 가렸고, 이전에는 CI maturin 단계가 이 이유로 실패했을 것이다.
- 커밋 `e38207f`의 골격은 rustfmt 기준에 맞지 않았다. 2026-09-25에 `cargo fmt`로 `instruction.rs`, `state.rs`의 기존 코드도 정렬되었다(동작 변화 없음).
- 정답 형식은 `gen9championsdoublescustomgame`이다. Champions 모드이고, 로스터 제한과 4마리 선출이 없으며, 레벨은 기본 50이다.

## 주의점과 함정

- **대상 번호는 양수=상대, 음수=아군이다.** `spread-damage`를 처음 만들 때 `highhorsepower -1`로 써서 아군 마기라스를 공격하는 실수가 있었고 `2`로 고쳤다. 새 시나리오는 로그로 대상을 확인한다.
- 필드·날씨·진영 효과 패치에는 source 포켓몬이 필요하다. `enumerate.cjs`는 p1 선두를 쓰고 남은 턴은 패치 값으로 덮어쓴다.
- Showdown은 더블 광역기(`allAdjacentFoes`, `allAdjacent`, `foeSide`)의 명목 대상을 무작위로 뽑는다. 결과와 무관한 난수라서 `full`에서는 접는다. 이 가정이 틀리지 않는지 `mc` 비교로 계속 확인한다. `randomNormal`(역린 등)은 접지 않는다.
- Champions 차이점:
  - 수면은 `sample([2,3,3])`이다.
  - 마비로 못 움직일 확률은 1/8이다.
  - 얼음은 최대 3턴이고 매 턴 1/4 확률로 녹는다.
  - PP 상한은 20이다.
  - 능력치는 HP = 종족값 + SP + 75, 나머지는 (종족값 + SP + 20) × 성격 보정이다(레벨 50, IV 반영 없음).
  - 자세한 구현은 `vendor/pokemon-showdown/data/mods/champions/{scripts,conditions}.ts`에 있다.
- 기믹은 `Ruleset`을 거쳐서만 허용한다. `Pokemon.gimmicks`(개체 자격)는 기본값이 비어 있다. `lab-scenario` 로더가 `gimmick::structural_gimmicks`로 메가 자격만 채운다. 손으로 만든 상태는 채우지 않으면 메가 행동도 `GimmickUnavailable`로 거부된다. 새 기믹을 켤 때는 enum을 바꾸지 말고 규칙셋에 추가한다.
- 정규 상태의 effect 필드는 허용 목록(`EFFECT_FIELDS`)이다. 불일치가 나오면 필드를 추가하고 `SCHEMA`를 올린다.
- `data/champions.json`(2.5MB)은 고정된 Showdown 커밋 `9e317a6`에서 생성했다. vendor를 갱신하면 `export.cjs` → `gen-rust.cjs` 순서로 다시 생성한다. CI에는 vendor가 없으므로(gitignore) CI는 `gen-rust.cjs --check`만 한다.
- dex 인덱스는 export가 바뀌면 달라진다. 규칙 코드는 숫자 대신 생성 상수(`moves::GRAVITY`)를 쓴다. 항목이 export에서 빠지면 컴파일 오류로 드러난다.
- 생성기가 일부러 버리는 필드(진화·알 그룹·색·태그·`requiredItem` 등 합법성/표시용)는 `gen-rust.cjs`의 `*_DROPPED`에 있다. 콜백 대신 상수 `false`인 이벤트(`onTakeItem: false` 등)는 bool 필드가 된다. 이벤트 순서 상수(`on*Priority/Order/SubOrder`)는 `event_orders`에 남긴다.
- `MoveTarget::User`는 Showdown의 `self` 대상이다(Rust 예약어 회피).
- **로더 상태는 선두 배치 직후, 등장 효과 전이다.** 등장 효과는 `switch_in::initial_outcomes`가 따로 펼친다. oracle `before`는 고정 시드 한 경로이므로 펼친 결과 **중 하나**와 같아야 한다(`single-hit`: 트레이스가 마기라스/몰드류 중 무엇을 복사했는지는 시드가 정한다). `state.turn`은 Showdown의 첫 결정 시점 값인 1로 둔다.
- **등장 펼치기의 특성 경계:** 동작 구현 = 트레이스(Trace), 모래날림(Sand Stream), 그래스메이커(Grassy Surge). 시작 시 무동작 확인 = 모래헤치기(Sand Rush). 나머지는 dex `handlers`에 시작 구간 이벤트가 없을 때만 무동작으로 받고, 있으면 `SwitchInError::UnsupportedAbility/Item/Species`로 거부한다(위협, 가뭄·잔비 등 다른 날씨/필드 특성, 에어록, 구애스카프(`onModifySpe`), 씨앗류·부스트에너지·풍선처럼 시작 시 발동하는 도구 등). 다른 날씨 특성을 넣을 때는 `set_weather`의 바위 대응(열기바위 등)과 날씨 덮어쓰기 규칙을 함께 구현한다.
- `Effect.turns`는 Showdown의 남은 `duration`과 같은 값으로 정했다(정규 출력 `weatherDuration` 등). `Effect::PERMANENT`는 정규 출력에서 아직 거부한다.
- `can_mega`: 정규 출력의 `canMega`는 개체 메가 자격 ∩ `format_ruleset(meta.format)` − 진영 메가 사용이다. M-C 메가 전용 잠금과 꺼 둔 기믹 구조는 바꾸지 않았다.
- 로더는 `hypnosis-gravity.json`을 **거부한다**(중력 `patch`가 있음). `single-hit`, `spread-damage`는 불러온다. 패치를 무시하고 불러오면 다른 국면이 되므로 일부러 막았다.
- Champions PP는 단순 상한이 아니다. 방어는 Champions 기본 PP가 5라서 최대 8이다(10이 아님). 기본 PP가 5의 배수가 아닌 기술은 모두 PP 증가 불가(Z/다이맥스기)이고, 이를 `state.rs` 테스트가 확인한다.
- 메가 자격은 Champions `canMegaEvo`처럼 `item.megaStone[species.name]`, 즉 정확한 종 일치로만 준다. `Gardevoir-Mega`가 가디안나이트를 들어도 자격이 없다. 레쿠쟈의 화룡점정 경로는 past/future 태그 규칙이 필요해서 모델링하지 않았다. `teraType`은 사이드카에만 보관하고 자격으로 주지 않는다(`Pokemon`에 테라 필드가 아직 없음).
- 로더가 받는 형식은 `gen9championsdoublescustomgame`뿐이다(모든 멤버 선출). `gen9championsvgc2026regmc`의 4마리 선출은 아직 없어서 거부한다. IV는 0–31 범위만 검사하고 버린다(Champions 공식에 IV 항이 없음). 이름은 Showdown처럼 20자로 자르고 편마다 유일해야 한다.

## 다음 할 일 (순서대로)

1. [완료 2026-09-25] `data/champions.json` → Rust 정적 테이블 (`core/src/dex/`).
2. [로더·초기 분포·canonical 완료 2026-09-25] 시나리오 JSON → `State<2>` 로더, 정규 상태 출력(`canonical.rs`), 첫 등장 펼치기(`switch_in.rs`: 트레이스·모래날림·그래스메이커). 남은 것: 다른 등장 특성·도구(위협, 날씨·필드 특성 전부, 씨앗류), `patch` 적용(HP·상태·수면 턴·랭크·진영·필드), `setupTurns`(턴 엔진 이후), VGC 4마리 선출.
3. [원시 연산 완료 2026-09-25] Champions 능력치 공식과 16롤 데미지 코어. 남은 것: 타입 상성·날씨·필드·도구·특성 훅을 `State<2>`와 연결하고 여러 오라클 시나리오로 검증한다.
4. 턴 진행: 우선도 → 스피드 → 동속 분기(행동마다 재정렬), 대상 해석과 유도, 방어, 속이기, 교체, 기절 후 교체, 턴 종료 순서. 턴 입력은 `Ruleset::validate_joint_action`을 통과한 행동만 받는다. 슬롯별 후보는 `Ruleset::joint_actions`로 넘긴다.
5. 기술 이식: 중력, 최면술, 수면, 날따름, 분노가루, 트릭룸, 날씨, 필드, 위협, 메가진화를 먼저 한다. 우선순위는 `teams/library` 더블 사용 빈도 × `handlers` 미구현 여부로 정한다.
6. 목표 지표: 시나리오 모음에서 정확 일치 99% 이상, 턴당 분포 열거 속도는 PokaiEngine 보고치(약 0.08ms)와 같은 자릿수.

## 참고 자료

- PokaiTrainer 논문 요약:
  - 규칙은 Reg M-B, 공개 팀시트 전제.
  - 선출은 90×90 행렬 게임으로 푼다.
  - 배분은 엔진 재분기 기반의 베이즈 갱신으로 추정한다.
  - 사람 래더 150세트에서 약 59% 승률.
  - 판단 시간 중앙값은 2.7초이고, 탐색 규모를 키우면 23초.
  - 한계: 초반 형세를 낙관적으로 평가하는 문제가 남아 있고, 비공개 팀시트 환경은 다루지 않는다.
- 요약 도구로 읽은 내용이므로 수치를 인용하기 전에 원문을 확인한다.
