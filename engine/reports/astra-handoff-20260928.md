# SIMD/CUDA 위임 검토 및 Astra 인계문 (2026-09-28)

요청: `engine/`의 SIMD/CUDA 최적화 일부를 외부 에이전트(Codex GPT-6 Astra)에 맡길 수 있는지 검토하고 인계문을 작성한다. 이 문서는 검토·인계문이며 구현이나 새 에이전트 시작은 아니다.

기준 커밋: `bd8f6e6`(웨이브 16 + 검증 웨이브 VA·VC·VD 병합). 이후 병합 예정: VB(러너 결과 표, `runs/`·문서만), VE(`engine/scripts/oracle_local.py`). 둘 다 `core/`·`search/`의 계산 코드를 만지지 않는다.

## 1. 결론 요약

| 후보 | 판단 | 근거 |
|---|---|---|
| ① `damage.rs::damage_rolls` SIMD | **지금 위임 가능 — 단, "측정 우선, 채택 조건부"** | 코드는 SIMD에 이상적(16개 독립 u32 롤)이지만 전체 시간 비중은 측정된 적이 없고 **1% 미만으로 추정**된다. 첫 작업의 산출물은 채택 여부와 무관하게 "정확한 비중 측정 + 자동 벡터화 여부 + 결과가 있으면 구현"이다 |
| ② `search/nash.rs` RM+ SIMD | **보류(측정 뒤)** | XX가 RM+ 내부 루프를 이미 최적화해 sand-owen deep-nash에서 자식 RM+ 합계가 1 s 미만(전체 20.5 s). 비트 동일성 테스트(`solve_is_bit_identical_to_the_cell_loop`)가 합산 순서를 고정한다. 순서를 보존하는 행 블록 벡터화는 가능하지만 이득 상한이 5% 미만 |
| ② CUDA 행렬 배치 | **보류** | RM+가 전체의 5% 미만이라 전송·동기화 비용을 이길 수 없다. 자식 행렬은 double oracle이 필요한 셀만 평가해 크기·수가 가변이다. 재검토 조건은 아래 §4 |
| ③ HP 분포 배치 연산 (P1e 연계) | **보류** | P1e(롤 지연화)·P7(분기점 fork)이 표현 자체를 바꿀 예정이라 지금 배치 연산을 넣으면 두 번 만든다. P1e/P7은 아직 미배정이지만 다음 성능 웨이브 항목이다 |

**전체 시간이 어디에 있는가(YY 최종 프로파일, sand-owen·psy-cona Median 스윕):** 규칙 코드 `run_stage` ~60%, 힙 할당 ~12%(`Vec<Instruction>` 로그·결과 diff·병합 복제), `Merger::add`의 `Eq` 확인·복제 ~13%, `diff::instructions` ~9%, `HistoryReaders::of` ~5%. **SIMD/CUDA로 줄일 수 있는 성분이 이 목록에 없다.** 데미지 계산은 "규칙 코드 60%" 안의 작은 조각이고, 나머지는 분기·할당·비교다. 따라서 탐색 시간을 실제로 줄이는 레버는 구조(P7 분기점 fork, 할당 제거, P1e)이며, SIMD 작업은 "확인하고 닫는" 성격이 강하다. 이 판단 자체를 Astra의 첫 산출물(측정)로 검증한다.

## 2. 후보 ①의 사실 관계 (인계 전 확인한 것)

- 함수: `engine/core/src/damage.rs:91` `pub fn damage_rolls(input: DamageInput) -> [u16; 16]`. 롤 루프(`while index < 16`)는 `base_damage.wrapping_mul(85+i) / 100` → STAB(`apply_rounded_modifier`, ×mod / 4096 반올림: 반 초과만 올림) → 타입 상성(×/4096, 절사) → 화상(/2) → final modifier(반올림) → protected(¼ 반올림) → `max(1)` → `as u16` 절단. 모든 곱은 `wrapping_mul`, 나눗셈은 상수(100, 4096)라 컴파일러가 곱셈-시프트로 바꾼다. **AVX2에는 32비트 `mullo`가 있어 자동 벡터화가 가능한 형태**이고, 실제로 됐는지는 릴리스 기계어로 확인해야 한다(`cargo rustc -p lab-engine --release -- --emit asm` 또는 `cargo asm`). 프로젝트의 릴리스 프로필과 `target-cpu` 설정(`engine/Cargo.toml`, `.cargo/config.toml` 유무)을 먼저 본다.
- 호출: `engine/core/src/turn/moves.rs:4175`에서 타격당 1회. 결과는 `turn/branch.rs::Chooser::roll`이 롤 모드(`Full`=16, `Extremes`=2, `Median`=1, …)에 따라 인덱스를 고른다. **Median 모드에서도 16개를 다 계산하고 하나만 쓴다** — SIMD보다 "필요한 인덱스만 계산"이 먼저일 수 있다(단, `Full`·인수분해 경로는 16개 전부 필요).
- 비중: 열거 1회 ≈ 0.44 ms(YY 뒤, 결과 ~31개)에 타격은 수 개~수십 개이고 `damage_rolls` 한 번은 수십 ns 수준으로 추정 → **열거 시간의 1% 안팎**. 추정이므로 측정이 첫 과제다.
- 기존 벤치 `engine/core/benches/damage.rs`: 2천만 회 호출하지만 **`black_box(rolls[15])`만 소비**해 나머지 15개 롤이 죽은 코드로 제거될 수 있다. 사용자 완료 기준 "벤치가 16개 결과 전체를 실제로 측정하는지"에 해당하는 결함이며, 인계 작업에 수정을 포함한다.
- 정확성 기준: `damage.rs` 단위 테스트(`champions_oracle_grassy_glide_rolls_match` 등), `scenario/tests/*`의 오라클 fixture 약 1,200개(피해 1이 다르면 분포가 달라져 실패), 말뭉치 1,781국면(`parity_corpus.py check`, 104 s). SIMD 경로는 이 셋을 전부 그대로 통과해야 한다.

## 3. 충돌·의존성

- `damage.rs`를 만지는 진행 중 작업은 **없다**. 검증 웨이브(VB·VE)는 `runs/`·스크립트만. P1e/P7은 미배정.
- `turn/moves.rs`의 호출부(4175)는 건드리지 않는다(규칙 코드, 여러 차선이 수정하는 파일). 인덱스 선택 최적화가 필요하면 `damage.rs`에 `damage_rolls_at(input, indices)` 같은 **추가 API**로 두고, 호출부 교체는 별도 항목으로 보고한다.
- 빌드 환경: 로컬 MSVC(`D:\VS\BuildTools`), `CARGO_TARGET_DIR`을 세션별로 분리(예: `D:/cargo-target-astra`). 다른 세션의 빌드가 CPU를 쓰므로 벤치는 **CPU 시간과 교차 실행**(`engine/scripts/search_bench.py --compare base=…,new=…`)으로 잰다.
- CI: `.github/workflows/engine.yml`이 `lab-engine`/`main` 푸시에서 fmt·clippy(`-D warnings`)·`cargo test --release`를 Linux·Windows 양쪽에서 돈다. 러너에 AVX2가 있더라도 **런타임 감지 + 스칼라 폴백**이 필수다(`is_x86_feature_detected!`, 비-x86에서는 폴백만).

## 4. 보드 항목과 파일 범위

| 보드 ID | 내용 | 변경 가능 파일 | 상태 |
|---|---|---|---|
| `SIMD1-damage-rolls-feasibility` (w2) | ① 측정 → 조건부 구현 | `engine/core/src/damage.rs`(추가 위주), 새 `engine/core/src/damage_simd.rs`(선택), `engine/core/benches/damage.rs`, `engine/core/Cargo.toml`(feature·dev-dep), `engine/core/tests/damage_simd.rs`(새 테스트), `runs/simd-<날짜>/README.md` | **위임 가능** |
| `SIMD2-rmplus-row-block` (w2) | ② RM+ 행 블록 벡터화(합산 순서 보존) | `engine/search/src/nash.rs`, `engine/search/tests/*` | 보류: 최신 프로파일에서 RM+ ≥ 5%일 때 |
| `CUDA1-batched-matrix-eval` (w3) | ② 자식 행렬 GPU 배치 | 새 크레이트(`engine/gpu/`) | 보류: S24-t7(교체 셀 풀이 방식) 결정 뒤, RM+ 비중이 커질 때 |
| `SIMD3-hp-distribution-batch` (w3) | ③ HP 분포 배치 연산 | `turn/lazy.rs`, `turn/frontier.rs` | 보류: P1e·P7 뒤 |

**채택 기준(공통):** (a) 스칼라 경로 유지, 미지원 CPU에서 동작; (b) 대표 입력 + 경계값(u16 최대 공격/방어, base_power 0, 면역, 상성 0, 화상+protected, wrapping이 실제로 걸리는 값)에서 16개 롤 전부 기준 구현과 비트 일치 — 무작위 10⁶ 입력 + 경계 격자; (c) fmt·clippy·`cargo test --release --workspace` 통과, 말뭉치 1,781국면 일치; (d) 같은 빌드 조건에서 기준/변경 교차 측정: 마이크로벤치(ns/호출, 16개 전부 소비)와 실제 열거·탐색(`search_bench.py` 3국면, CPU 시간)을 분리 보고; (e) **탐색 CPU 시간이 잡음(±3%)을 넘어 줄 때만 채택**, 아니면 "측정 결과와 미채택 사유"를 `runs/simd-<날짜>/README.md`에 남기고 코드는 feature-gated로 두거나 제거.

## 5. 추론 수준 의견

- SIMD1: **high**가 적절하다. 코드가 100줄 미만이고 위험은 정확성(반올림·wrapping)과 측정 설계에 있다. xhigh는 과하다.
- SIMD2/CUDA1/SIMD3: 지금은 착수 자체를 보류하므로 수준을 정할 필요가 없다. 착수한다면 CUDA1·SIMD3은 표현 설계가 들어가므로 xhigh가 맞다.

## 6. Astra 인계 프롬프트 (그대로 전달)

```
# lab-engine: damage_rolls SIMD 타당성 측정과 조건부 구현 (보드 SIMD1-damage-rolls-feasibility)

## 맥락
- 저장소 D:\pokemon-ai-lab, 브랜치 lab-engine, 기준 커밋 bd8f6e6. 작업은 새 브랜치 `astra-simd1`에서 하고 커밋마다 `git push origin astra-simd1`. lab-engine에는 푸시하지 않는다.
- 이 프로젝트는 Pokémon Champions 규칙의 Rust 전투 엔진(engine/core)이다. 정확성은 Showdown 오라클 fixture와 실전 말뭉치로 검증하며, 데미지 1의 차이도 결과 분포를 바꾸어 테스트가 실패한다. 먼저 읽을 것: engine/CONTEXT.md("실행 방법"), engine/reports/astra-handoff-20260928.md §2–§4, engine/core/src/damage.rs, engine/core/src/turn/branch.rs의 Chooser::roll, engine/core/benches/damage.rs.
- 빌드: MSVC, `CARGO_TARGET_DIR=D:/cargo-target-astra`, 항상 `--release`. 다른 세션이 CPU를 쓰므로 시간은 CPU 시간과 교차 실행으로 잰다.

## 목표
SIMD 사용이 목표가 아니다. 목표는 "damage_rolls가 탐색 시간에서 차지하는 비중을 측정하고, 벡터화가 실제로 탐색을 빠르게 하면 채택, 아니면 근거를 남기고 미채택"이다.

## 단계
1. 측정 설계 수정: engine/core/benches/damage.rs가 rolls[15]만 소비해 나머지 15개가 제거될 수 있다. 16개 전부를 소비(합계 또는 XOR을 black_box)하도록 고치고, 대표 입력 4종(단일 타격 중립, STAB+상성 2배, 광역+화상+protected, 급소+날씨)을 돌려 ns/호출을 기록한다.
2. 비중 측정: 실전 국면(D:/pokemon-ai-lab/runs/plan-20260926/gardevoir-vs-sand-owen.json --position 1 등 engine/scripts/search_bench.py의 기준 3국면)에서 열거·탐색 중 damage_rolls 호출 횟수와 누적 시간을 잰다(카운터를 cfg(feature)로 넣거나 samply/perf 프로파일). 결과: 열거 1회당 호출 수, 전체 CPU 시간 대비 비중(%).
3. 자동 벡터화 확인: 릴리스 기계어(`cargo rustc -p lab-engine --release -- --emit asm`)에서 damage_rolls 루프가 이미 벡터 명령(vpmulld 등)으로 나오는지 확인하고 기록한다. Cargo.toml/.cargo/config.toml의 target-cpu·opt-level도 기록한다.
4. 판단: 2의 비중이 2% 미만이면 구현하지 않고 5로 간다. 2% 이상이면 구현한다:
   - engine/core/src/damage.rs에 스칼라 경로를 그대로 두고, x86_64 AVX2 경로를 `is_x86_feature_detected!("avx2")` 런타임 분기 + `#[target_feature(enable = "avx2")]` unsafe 함수로 추가(std::arch, 외부 크레이트 없음). 비-x86과 미지원 CPU는 스칼라.
   - 각 단계(×random/100, apply_rounded_modifier의 "반 초과만 올림", 상성 ×/4096 절사, 화상 /2, final, protected ¼, max(1), u16 절단)를 wrapping 의미까지 그대로 유지한다. 나눗셈은 컴파일러가 하듯 곱셈-시프트로 바꾸되 결과가 모든 u32 입력에서 정확히 같아야 한다(증명 또는 전수 근거).
   - 호출부(engine/core/src/turn/moves.rs:4175)는 바꾸지 않는다. Median 등 단일 인덱스만 필요한 경우를 위한 `damage_rolls_at(input, &[usize])` 같은 추가 API는 만들어도 되지만 호출부 교체는 보고서에 제안만 한다.
5. 정확성 검증: 새 테스트 engine/core/tests/damage_simd.rs — 무작위 입력 10^6개(모든 필드를 전 범위에서, wrapping이 실제로 걸리는 큰 값 포함) + 경계 격자(base_power 0, attack/defense 0·1·65535, type_effectiveness 0·4096·8192·16384, burned/protected/critical/spread/parental_bond 조합)에서 SIMD와 스칼라의 16개 롤 전부 비트 일치. 그리고 `cd engine && cargo fmt --all -- --check && cargo clippy --release --workspace --all-targets -- -D warnings && cargo test --release --workspace`, 말뭉치 재대조 `PYTHONUTF8=1 LAB_ROOT=D:/pokemon-ai-lab python engine/scripts/parity_corpus.py check runs/parity-corpus-20260927 --jobs 2 --check D:/cargo-target-astra/release/lab-check.exe --out <scratch>`가 1,781/1,781 일치.
6. 성능 검증: 같은 빌드 조건에서 기준(bd8f6e6)과 변경을 교차 실행. 마이크로벤치(ns/호출)와 실제 탐색(`PYTHONUTF8=1 python engine/scripts/search_bench.py --plan-dir D:/pokemon-ai-lab/runs/plan-20260926 --out-dir runs/simd-<날짜> --compare base=<기준 lab-plan.exe>,new=<변경 lab-plan.exe> --stats-for new`, 6스레드와 1스레드)을 분리해 CPU 시간으로 보고. 측정 중 다른 프로세스의 CPU 부하(작업 관리자 또는 `Get-Process` CPU 합)를 함께 적는다.
7. 채택 기준: 탐색 CPU 시간이 ±3% 잡음을 넘어 줄어들 때만 기본 경로로 채택. 아니면 feature `simd-damage` 뒤에 두거나 제거하고, 측정 결과·기계어 확인·미채택 사유를 runs/simd-<날짜>/README.md에 남긴다.

## 하지 말 것
- engine/core/src/turn/*, engine/search/*, engine/scenario/* 수정(다른 작업이 진행 중). 필요하면 보고서에 제안.
- 합산 순서·반올림·wrapping 의미 변경. f32/f64 도입.
- 외부 SIMD 크레이트 추가(std::arch만). CUDA 착수(보류 항목).
- 로컬 lab-engine 브랜치 변경, 말뭉치 `runs/parity-corpus-20260927` 안에 결과 쓰기(별도 디렉터리 사용).

## 보고 형식
브랜치·커밋 목록, 단계 1–3의 수치(ns/호출, 열거당 호출 수, 비중 %, 자동 벡터화 여부·명령어), 구현 여부와 근거, 정확성 결과(테스트 수, 말뭉치 1,781 일치 여부), 성능 표(기준/변경, 마이크로/실제, 1·6스레드, CPU 부하), 채택/미채택 결정, 다음 후보 추천(측정에 근거해: 예를 들어 "비중 0.6%라 SIMD 무의미, 할당 12%가 다음"). 하지 않은 것을 했다고 쓰지 않는다.
```
