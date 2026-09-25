# lab-engine 설계

더블(VGC)을 기본으로 하되 싱글도 손해 없이 쓰는 탐색용 배틀 엔진. 현재는 **골격 단계**다. 규칙 처리(턴 진행·데미지·기술 효과)는 아직 없다.

## 왜 직접 만드나 (2026-09-25 조사)

| 후보 | 탈락 사유 |
|---|---|
| PokaiEngine (arXiv 2608.29197) | 더블·M-B·턴당 0.08ms로 가장 가깝지만 **미공개**(Showdown 운영진과 공개 조율 중), 싱글 미지원 |
| poke-engine (pmariglia) | 빠르지만 싱글 전용. 랭크·volatile이 Side에 붙어 있고 행동자 2명 전제, `MoveTarget`은 User/Opponent뿐 |
| Showdown | M-C 정확, 싱글·더블 공통. 턴당 약 2ms로 탐색에 느림 → **정답(oracle)으로 사용** |
| battler, pokemon-showdown-rs, pkmn/engine | Champions 미지원, 탐색용 make/unmake 없음, 또는 Gen 1–2만 |

전제: 탐색 예산이 늘수록 실력이 오른다. 그래서 엔진 속도가 병목이다.

## 핵심 원칙

1. **슬롯 수 = const generic `N`.** `State<1>` 싱글, `State<2>` 더블. 형식별로 따로 컴파일되므로 싱글이 더블 지원 비용을 내지 않는다. 광역 0.75배처럼 "실행 시점 대상 수"로 정의된 규칙은 N=1에서 저절로 싱글 규칙이 된다. 저절로 맞지 않는 예외만 형식 분기로 명시한다.
2. **make/unmake.** 턴 결과는 `Vec<Outcome { probability, instructions }>`. 각 `Instruction`은 되돌릴 정보를 담는다. 탐색은 상태를 복사하지 않는다(poke-engine 방식 계승).
3. **영구 상태와 슬롯 상태 분리.** HP·상태이상·도구·PP는 `Pokemon`(파티), 랭크·volatile·대타는 `Slot`.
4. **필드 효과는 테이블.** `field[FieldEffect]`, `side.effects[SideEffect]`에 `{value, turns}`. 중력 같은 효과를 추가할 때 enum variant만 늘린다.
5. **행동은 슬롯 단위.** `SlotAction`(기술+대상/교체), 진영 결정은 `[SlotAction; N]`. 탐색이 슬롯별 통계(factored DUCT)와 가지치기를 쓸 수 있게 한다. N=1이면 기존 DUCT와 같다.
6. **평가자는 trait.** `Evaluator<N>`로 휴리스틱·선형·신경망 평가자를 교체할 수 있다.
7. **대상 표기는 Showdown과 같다.** 양수=상대 위치, 음수=아군 위치, 0=지정 없음.

## 계층 (예정 포함)

```
core/
  state.rs        State<N> / Side<N> / Slot / Pokemon           [골격 있음]
  instruction.rs  Instruction, apply/reverse, Outcome            [골격 있음]
  field.rs        FieldEffect / SideEffect / Effect 테이블       [골격 있음]
  action.rs       SlotAction, JointAction<N>                     [골격 있음]
  eval.rs         Evaluator<N>, Material                         [골격 있음]
  data/           종·기술·특성·도구 테이블 (Showdown data에서 생성)  [예정]
  turn/           행동 큐(우선도→스피드, 동속 확률 분기, 행동마다 재정렬),
                  대상 해석(생존 슬롯, 대상 유도, 광역 여부), 훅, 턴 종료  [예정]
  search/         슬롯 분리 DUCT, PUCT 사전확률, 결정화 샘플 병렬       [예정]
py/               pyo3 바인딩 (abi3, Python 3.12+)
```

## 검증 계획

- **정확성:** Showdown(`vendor/pokemon-showdown`, M-C)과 차분 테스트. 같은 상태·행동에서 결과 분포(데미지, 순서, 필드 변화) 비교. 싱글은 poke-engine 테스트를 정답 테스트로도 쓴다.
- **속도:** poke-engine `data/benchmark.rs`와 같은 상태로 초당 노드 수 비교. 싱글 모드 ±5% 이내 목표.
- **실력:** 싱글에서 새 엔진 foul-play와 원본 foul-play의 쌍 비교 대전.
- 기술 이식 순서는 `teams/library`의 사용 빈도 × 미구현 여부(커버리지 감사)로 정한다.

## 빌드

로컬 빌드는 하지 않는다. `.github/workflows/engine.yml`이 GitHub Actions에서 수행한다.

- `core`: fmt, clippy, `cargo test -p lab-engine` (Ubuntu, Windows)
- `wheels`: lab-engine wheel (maturin, abi3)
- `poke-engine-baseline`: poke-engine `f4e224c`(0.0.48, foul-play 고정 버전)의 `terastallization`(Gen 9 싱글)·`bss`(Champions BSS) wheel. 싱글 기준선용.

결과 wheel은 Actions artifact에서 받는다.
