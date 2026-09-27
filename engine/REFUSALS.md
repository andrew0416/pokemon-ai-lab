# 턴 엔진 거부 목록 (자동 생성)

`python engine/scripts/refusals.py`가 `engine/core/src/turn/`의 거부 지점(`b.unsupported(...)`, `TurnError::Unsupported`, FF의 `Battle::refused`, `check_state`와 정적 지원 검사 등 메시지를 만드는 함수)을 소스에서 뽑고 `engine/scripts/refusals.classification.json`의 분류를 붙여 만든다. 직접 편집하지 않는다. `--check`는 새 거부가 분류 없이 추가되거나 분류가 낡으면 실패한다.

범위: Champions 모드(Showdown `9e317a6`)에서 `isNonstandard`가 null인 종(메가 포함, 배틀 중 폼은 기본 종이 표준일 때만), 그 종의 특성(+심플빔·고민씨가 주는 심플·불면), `learnsets.ts`의 기술(+표준 플래그 기술·발버둥), 표준 도구. 테라·다이맥스·Z는 규칙셋이 막는다. 목록은 `refusals.universe.json`, 근거 검사는 아래 '근거'.

- 거부 호출 57곳(함수 42개). 키 95개 = 호출에 쓰인 메시지 49개 + 다른 함수의 메시지를 전달하는 호출 6개 + 메시지 생산 함수(`producers`)의 메시지 40개.
- 도달 가능 11개, 도달 불가능 84개, 미확인 0개.

## 도달 가능 (Champions 표준 범위)

| 키 | 지점 | 종류 | 재현 시나리오 | 오라클 결과 수 | 보드 | 이유 |
|---|---|---|---|---|---|---|
| `{} switching into hazards whose order (Showdown effectOrder) decides the outcome` | conditions.rs::entry_hazards | mechanic | `rr-hazard-order` | full: 1 | R2-hazard-effect-order | Stealth Rock / Spikes with Toxic Spikes (or Sticky Web against Mirror Armor, Corviknight) on one side and a newcomer the damage can knock out: Showdown runs them in the order they were set (a Poison type knocked out first leaves the Toxic Spikes), which the state does not keep. |
| `{} hitting a holder of {}` | moves.rs::future_move_hit | mechanic | `rr-future-sight-red-card` | full: 10 | R5-future-move-edges | Future Sight hitting a Red Card holder: the card drags the user out after the residual. (Eject Button ignores future moves and is no longer refused: fixed in this audit, `rr-future-sight-eject-button`.) |
| `{} of {} hitting after its user left the field, {}` | moves.rs::place | mechanic | `uu-future-sight-absent-user-unnerve` | full: 6 | R5c-future-move-absent-user-occupant | The benched user of a future move stands in for the hit in a position of its side (`moves::AbsentUser`); a hit whose outcome the displaced occupant could change is refused: an occupant with ability or item handlers acting for other Pokémon (`onAny*`, `onAlly*`, `onFoe*`: Unnerve against the target's berry, the Ruin abilities, Battery, ...) or suppressing abilities or the weather, and a target whose handlers act on an active source or on every active Pokémon (Innards Out, Cotton Down, Gulp Missile, Rowap Berry). The prefix rule is conservative (Friend Guard or Shadow Tag change nothing here). |
| `Beat Up with benched allies of different power {} (their order in Showdown's side.pokemon depends on the switches so far, which the state does not keep)` | moves/handlers.rs::beat_up_powers | mechanic | `rr-beat-up-bench` | extremes: 28 | R9-beat-up-order | Beat Up (standard) with two eligible benched allies of different base Attack: the hit order follows `side.pokemon`, which switches reorder. |
| `{} after the battle start (its once-per-battle flag is not in the state)` | switching.rs::once_per_battle | mechanic | `rr-supersweet-syrup-switch` | full: 1 | R10-once-per-battle-flags | Supersweet Syrup (Hydrapple) starting after the battle start (a switch-in, a Skill Swap); `pokemon.syrupTriggered` is not in the state. Intrepid Sword and Dauntless Shield are not standard. |
| `Attract between {} and {} with an undecided gender (give the sets a gender)` | conditions.rs::attract_fails | input | `rr-cute-charm-undecided-gender` | full: 2 | R13-attract-gender | Cute Charm (Clefable, Milotic, Lopunny, Wigglytuff) next to a set without a gender: Showdown drew the gender at team creation (`battle.sample(['M', 'F'])`), the scenario does not say which. |
| `Rivalry next to {} of undecided gender (give the set a gender)` | abilities.rs::rivalry_problem (producer) | input | `rr-rivalry-undecided-gender` | full: 22 | R13-attract-gender | Rivalry (Luxray, Pyroar) on the field next to a set without a gender. |
| `check_turn: -> support::check_state(state)` | mod.rs::check_turn | forward | `rr-rivalry-undecided-gender` | full: 22 | R13-attract-gender | Forwards `check_state`: reachable through the Rivalry gender check (R13); its `{}: ability {} ({})` is not since R1 (Trace on the field is supported). |
| `switch_in_as: -> why` | switching.rs::switch_in_as | forward | `rr-rivalry-switch-in-undecided-gender` | full: 1 | R13-attract-gender | Forwards `switch_in_problem`: reachable only through its Rivalry gender message; the others are E1, E2, E3, E8. |
| `{} with {} of undecided gender (give the set a gender)` | moves/handlers.rs::try_immunity_problem | input | `rr-attract-undecided-gender` | full: 1 | R13-attract-gender | Attract (standard) between sets without a gender (Captivate is not standard). |
| `{}: Rivalry next to a Pokémon of undecided gender` | switching.rs::switch_in_problem (producer) | input | `rr-rivalry-switch-in-undecided-gender` | full: 1 | R13-attract-gender | A Rivalry holder switching in next to a set without a gender. |

## 고친 거부 (소스에서 사라짐)

| 이전 키 | 보드 | 오라클 fixture | 내용 |
|---|---|---|---|
| `Instruct repeating {} (its lastMoveTargetLoc is not kept)` | R6-instruct | `rr-instruct-target` (full: 1) | `Slot::last_move_target_loc` (`Instruction::SetLastMoveTargetLoc`, hidden, recorded while an Instruct is in a party) keeps `moveUsed`'s target location; the instructed action aims at it (also `o38-instruct-target`). |
| `Instruct on a Quick Claw holder` | R6-instruct | `rr-instruct-quick-claw` (full: 1) | Quick Claw's fractional priority is drawn again for the instructed action (see Quick Draw). |
| `Instruct on a Quick Draw holder` | R6-instruct | `rr-instruct-quick-draw` (full: 2) | Instruct's `resolveAction` runs `FractionalPriority` for the new action (Quick Draw, Quick Claw, Custap Berry, the constants) as the turn's first stage does. |
| `Instruct repeating {}, which the target does not know` | R6-instruct | `nn-instruct-called-raging-fury` (full: 32) | A last move outside the slots became reachable with R21 (Copycat calling Raging Fury, which is not `failinstruct`, locks its user into it). The instructed action holds the move id: `runMove` spends no PP while the user is locked and fails with `cant nopp` otherwise (`deductPP` finds no slot). |
| `Encore replacing a queued action with {} (a callback action it would queue)` | R8-encore-edges | `rr-encore-counter` (full: 1) | The Champions Encore's `changeAction` also queues the encored move's `beforeTurnMove` (Counter, Mirror Coat) or `priorityChargeMove` (Focus Punch, Beak Blast, Chilly Reception) action, which runs next (`Battle::encore_change_action`; `nn-encore-counter-hit`, `nn-encore-focus-punch-hit`, `nn-encore-beak-blast-hit`); the new action's target is drawn when it is queued. `Battle::refused` (FF) and its `stage_end_check` forward are gone. |
| `Sleep Talk calling {} (a multi-hit move)` | R14-called-multi-hit | `rr-sleep-talk-multihit` (full: 1) | A multi-hit move Copycat or Sleep Talk calls suspends after its first hit like a chosen one; the caller's hit loop stops after its `spreadMoveHit` (`moves::CallerFrame` in the called move's `MoveProgress`) and, once the called hits and `useMoveInner` tail are done, goes on with its hit-loop tail, its own `useMoveInner` tail and AfterMove with the called move active (`moves::finish_called`; `nn-sleep-talk-double-hit-life-orb`, `nn-copycat-bullet-seed`, `rr-copycat-multihit`). |
| `a two-turn move` | R21-copycat-called-moves | `rr-copycat-two-turn` (full: 1) | Copycat calling a charge move: `twoturnmove` starts aimed at the drawn target (`getLocOf(defender)` for a called move, `handlers::charge_try_move`) and the second turn runs the locked move the user does not know, without PP (`nn-copycat-solar-beam-lock`). |
| `a lock on the called move` | R21-copycat-called-moves | `rr-copycat-outrage` (extremes: 81) | Copycat calling Outrage, Petal Dance, Thrash, Raging Fury (or Uproar, which was let through and then rejected as an unknown locked move): the caller's user is locked into the called move, which its next actions run without PP (`ActionKind::Move` holds `action.moveid`; `lock::queued_move_id`; `nn-copycat-outrage-lock`, `nn-copycat-uproar-lock`). |
| `queue actions of its own` | R21-copycat-called-moves | `rr-copycat-mirror-coat` (full: 1) | Copycat calling Mirror Coat or Chilly Reception: a called move queues no action (`useMove` resolves none), so Mirror Coat fails in `onTry` without its condition and Chilly Reception needs none. |
| `{}: no PP left when used` | R15-no-pp-when-used | `rr-spite-no-pp` (full: 1) | Spite / Eerie Spell taking the last PP after the choice: now Showdown's `cant ... nopp` (the move fails, no `lastMove`, no MoveAborted) in `moves::run_move_inner`. |
| `Toxic Spikes poisoning a Synchronize holder (Synchronize ignores Toxic Spikes)` | R2-hazard-effect-order | `rr-toxic-spikes-synchronize` (full: 1) | `Battle::try_set_status_from_toxic_spikes` skips Synchronize's `onAfterSetStatus` (`effect.id === 'toxicspikes'`). |
| `{} hitting a holder of {} (Eject Button)` | R5-future-move-edges | `rr-future-sight-eject-button` (full: 10) | Eject Button ignores future moves (`!move.flags['futuremove']`): the hit loop skips it for a future hit; only Red Card stays refused under the same message. |
| `Instruct repeating {}, which the target does not know (Struggle, Transform)` | R6-instruct | `rr-instruct-struggle` (full: 29) | Instruct checks the last move's flags (`failinstruct`, charge, recharge, Z, Max) before looking for its slot, as Showdown does; the refusal stays for a last move outside the slots that the flags do not fail, which no standard battle has (E11). |
| `Trace has no traceable foe and would keep seeking on later Updates` | R1-trace-seeking | `rr-trace-no-traceable` (full: 1) | Trace's `effectState.seek` is the hidden volatile `traceseek` (`Volatile::TraceSeek`, set by `switching::trace` when nothing was copied); every later Update in which its Trace acts copies a random traceable foe (`switching::trace_update` from `update::update_event`, before the item's handlers; a Speed tie is drawn while a Trace can copy). Also fixtures `mm-trace-*`. |
| `{}: Trace still seeking a target` | R1-trace-seeking | `rr-trace-gastro-acid-baton-pass` (full: 2) | Removed from `check_state` (it was shadowed by `{}: ability {} ({})`): Trace is supported on the field, seeking or not; a Trace that never started (Gastro Acid passed by Baton Pass) stays inert, as in Showdown (Gastro Acid lasts while its holder stays). |
| `redirection tie between {} and {} (Showdown breaks it by effectOrder)` | R4-redirection-tie | `rr-redirect-tie` (full: 1) | Two redirectors of one priority at equal Speed: Showdown sorts the RedirectTarget handlers with `compareRedirectOrder` in a stable sort (no tie shuffle), so the holder whose `abilityState.effectOrder` is lower (switched in or last had an ability set first) wins, whatever the move and the order of use. New hidden `Slot::ability_order` (restarted by switch-in, `setAbility`, Skill Swap, Transform, Mega Evolution and other permanent forme changes; carried by Ally Switch), recorded only while a redirector can be in the battle; also `oo-redirect-tie-swapped`, `oo-redirect-tie-worry-seed`, `oo-lightningrod-tie`. |
| `{} aimed at a side whose Pokémon Ally Switch swapped (it tracks its original target)` | R7-ally-switch-target | `rr-ally-switch-snipe-shot` (full: 17) | A tracking move (Snipe Shot; any move of a Stalwart or Propeller Tail holder) aimed at a Pokémon Ally Switch moved: the queued move action now holds Showdown's `originalTarget` (`resolveAction`: the Pokémon at `targetLoc` when queued) and `moves::get_target` aims at it while it is active (`getTarget`), else at the position. Also `ally-switch-snipe-shot`, `s-stalwart-ally-switch` (fixtures, were refused), `oo-ally-switch-stalwart` (Archaludon), `oo-snipe-shot-target-switched` (a target that left the field: the position). |
| `Recycle restoring {} (its onStart)` | R11-item-restart | `rr-recycle-seed` (full: 1) | Recycle is `pokemon.lastItem = ""; pokemon.setItem(item, source, move)`: the item is held again and `setItem`'s Start runs for every item with an `onStart` (`handlers::trick_item_start`, which Trick already used: Seeds, Room Service, White Herb, Metronome, the Choice items, Booster Energy, Utility Umbrella, Air Balloon). Also `oo-recycle-white-herb`, `oo-recycle-metronome`. |
| `Fling's user fainted before its item was thrown` | R16-fling-user-fainted | `rr-fling-innards-out` (full: 1) | Showdown's `eachEvent('Update')` in the hit loop (sim/battle-actions.ts:967) still holds the 0-HP user (its faint is processed after the loop), so Fling's condition `onUpdate` runs on it: `setItem('')` fails (`!this.hp`) and the item stays, `lastItem` is set, AfterUseItem runs (an ally's Symbiosis takes its item back when `setItem` fails on the fainted user), `removeVolatile` fails. `update::update_event` adds such users to the Update (`conditions::fling_update_fainted`); the never-firing guard in `moves::run_move_inner` is gone. Also `oo-fling-innards-out-symbiosis`. |
| `Trick-or-Treat's Curse Glitch (a queued Curse of the Ghost-typed target)` | R17-trick-or-treat-curse | `rr-trick-or-treat-curse-glitch` (full: 2) | Trick-or-Treat's `onHit` sets `action.targetLoc = -1` for a queued Curse of a target in the second position (data/moves.ts:19928; Champions only makes the move standard): the queued action now aims at the ally position; the Ghost Curse's ModifyMove turns an ally target into `randomNormal` and useMove draws a random foe (`handlers::on_modify_move`, already implemented). Also `oo-trick-or-treat-curse-first-slot` (no glitch in the first position). |
| `{} eaten by force while its holder ignores its item` | R22-forced-eat-ignored-item | `rr-teatime-klutz` (full: 1) | `eatItem(true)` (sim/pokemon.ts:1768) for a holder that ignores its item: `singleEvent('Eat')` is suppressed (sim/battle.ts:607: item handlers but Start, TakeItem and SetAbility), the berry is still consumed with `lastItem` and AfterUseItem. `update::eat_item_forced` skips `berry_on_eat` for such a holder instead of refusing. Also `oo-teatime-magic-room`. |
| `{}: {} would confuse a {} nature (confusion is not implemented)` | B36-refusal-messages | `oo-figy-berry-confusion` (full: 24) | Stale: confusion is implemented and `update::berry_on_eat` already confuses the eater of a Figy-type berry its nature dislikes (the Bug Bite / Pluck path was checked by `pluck-incinerate`). `update::berry_problem` is gone (with its call in `support::check_state` and `switching::switch_in_problem`); the holder's own Update eating matches the oracle (`oo-figy-berry-confusion`: Figy Berry, Timid Snorlax). The Figy-type berries are not standard. |
| `Emergency Exit of a replacement hit by entry hazards` | R3-emergency-exit-replacement | `rr-emergency-exit-replacement` (full: 1) | A replacement can suspend: `runAction('runSwitch')`'s tail (checkFainted, Update) checks Emergency Exit for the action's own Pokémon, the first queued `runSwitch` (fastest newcomer, Speed ties at random), and the switch request it raises suspends `enumerate_replacements`; `resume_turn` then only runs `endTurn` (`uu-emergency-exit-replacement-resume`, `uu-emergency-exit-two-replacements-tie`, `uu-emergency-exit-two-replacements-fast`). |
| `{} of {} hitting after its user left the field` | R5-future-move-edges | `rr-future-sight-user-left` (full: 20) | The benched (switched-out or fainted) user is seated in a position of its side for the hit only (`moves::AbsentUser`): a fresh slot (no stages or volatiles), its ability and item cleared (`ignoringAbility`/`ignoringItem` for an inactive Pokémon), everything restored after the hit (nothing the hit does to an inactive source lands). Also `uu-future-sight-user-left-item` (Twisted Spoon, Regenerator, a Calm Mind stage ignored) and `uu-future-sight-user-fainted` (fainted user, empty position). |

## 미확인

없음.

## 도달 불가능 (이유별)

### past-only: needs content that only non-standard (Past / CAP / LGPE / G-Max) sets have

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `Dancer copying {}: a multi-hit move` | moves.rs::run_external_move | Dancer is not standard (E8). | R20-generic-guards |
| `Gluttony restarting after Neutralizing Gas (its abilityState.gluttony = false)` | abilities.rs::neutralizing_gas_end | Needs Neutralizing Gas (E8). | R20-generic-guards |
| `Mirror Move calling {}` | moves/handlers.rs::on_try_hit | Mirror Move is not standard (E8); nothing calls it (Copycat copies moves used on the field). | R20-generic-guards |
| `Nature Power calling {}` | moves/handlers.rs::on_try_hit | Nature Power is not standard (E8). | R20-generic-guards |
| `Order Up from a commanded Dondozo whose Tatsugiri is gone (the source of `commanded` is not in the state)` | moves/handlers.rs::after_move_secondary_self | Order Up, Tatsugiri and Dondozo are not standard (E8). | R20-generic-guards |
| `Power Construct making a Zygarde holding {} able to Mega Evolve` | forme.rs::power_construct | Zygarde is not standard (E8). | R20-generic-guards |
| `Power Construct on {}` | forme.rs::power_construct | Zygarde is not standard (E8). | R20-generic-guards |
| `Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the suppressor's End WeatherChange)` | abilities.rs::paradox_suppressor_problem (producer) | Protosynthesis and Flower Gift are on no standard species and cannot be copied (E8). | R20-generic-guards |
| `Relic Song: Meloetta changing forme after fainting` | moves/handlers.rs::after_move_secondary_self | Relic Song and Meloetta are not standard (E8). | R20-generic-guards |
| `Shields Down on {}: the core colour (the set's species) is not in the state` | forme.rs::shields_down | Minior is not standard (E8). | R20-generic-guards |
| `Trace copying {} (cantsuppress: setAbility fails and Trace keeps seeking)` | switching.rs::trace_copy | Every standard `cantsuppress` ability (Battle Bond, Disguise, Stance Change, Zero to Hero) is also `notrace`, so Trace never picks one (E6). | R1-trace-seeking |
| `Trace next to No Ability` | switching.rs::trace | No standard species has No Ability and nothing sets it (E8). | R1-trace-seeking |
| `Zygarde-Complete fainting (Power Construct's formeRegression to the set's forme)` | battle.rs::faint_messages | Zygarde is not standard (E8). | R20-generic-guards |
| `a Rollout lock on the called move` | moves/handlers.rs::called_move_problem (producer) | Rollout and Ice Ball are not standard (E8); the target location their lock keeps for a called move is unverified. | R21-copycat-called-moves |
| `a switch request for {} slot {} (Eject Pack) during {}` | mod.rs::refuse_switch_request | During the battle start or a replacement only Eject Pack (not standard, E8) raises a switch request before the tail's Emergency Exit check (R3, which suspends a replacement). | R20-generic-guards |
| `field effect #{} (value {})` | support.rs::check_state (producer) | Only the primal weathers are outside sun / rain / sand / snow, and their holders are not standard (E8). | R20-generic-guards |
| `field effect #{} without a duration` | support.rs::check_state (producer) | Only the primal weathers are permanent (E8); every standard weather, terrain and room has a duration. | R20-generic-guards |
| `two Dancers with the same Speed (Showdown orders them by abilityState.effectOrder)` | abilities.rs::dancers | Dancer (Oricorio) is not standard, and copying it needs a holder on the field (E8). | R20-generic-guards |
| `{} called by {}: a multi-hit called move` | moves.rs::call_move | Copycat's and Sleep Talk's called multi-hit moves suspend between their hits (the caller waits in `CallerFrame`; fixed: `rr-copycat-multihit`, `nn-copycat-bullet-seed`); only Mirror Move, which calls from `onTryHit` and is not standard (E8), is still refused. | R14-called-multi-hit |
| `{} restarting after Neutralizing Gas at 0 HP` | abilities.rs::neutralizing_gas_end | Neutralizing Gas is on no standard species and is `notransform` / `failroleplay` / `failskillswap` / `noentrain` / `noreceiver` (E8). | R20-generic-guards |
| `{}: Battle Bond (its once-per-battle `bondTriggered` is not in the state)` | forme.rs::field_problem (producer) | Only Greninja-Bond and Greninja-Ash (not standard, E8) are refused; standard Greninja's Battle Bond does nothing. | R10-once-per-battle-flags |
| `{}: Mirror Herb keeps copied boosts past the end of a stage (its effectState persists until the next trigger)` | items.rs::stage_end_check | Mirror Herb is not standard (E8). | R20-generic-guards |
| `{}: Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the suppressor's End WeatherChange)` | switching.rs::switch_in_problem (producer) | Protosynthesis and Flower Gift are on no standard species and cannot be copied (E8). | R20-generic-guards |
| `{}: Utility Umbrella's `inactive` item state past the end of a stage (its onUpdate has not run)` | items.rs::stage_end_check | Utility Umbrella is not standard (E8). | R20-generic-guards |

### static: the support tables (COVERAGE.md, the evidence checks): no standard content reaches it

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `Copycat calling {}` | moves/handlers.rs::on_hit | Copycat (standard) calls `battle.lastMove`, and `called_move_problem` refuses no standard move any more: `move_unsupported` refuses none (E1), Rollout and Ice Ball are not standard (E8), and Beak Blast, the one standard move with an onAfterMove unchecked for a called move, is `failcopycat` (E12). Called two-turn, locking and queue-action moves run (fixed: `rr-copycat-two-turn`, `rr-copycat-outrage`, `rr-copycat-mirror-coat`). | R21-copycat-called-moves |
| `Cud Chew eating {}` | abilities.rs::cud_chew_residual | Every standard berry's onEat is implemented or empty (E4). | R20-generic-guards |
| `Fling feeding {}` | moves/handlers.rs::on_hit | Every standard berry's onEat is implemented or empty (E4). | R20-generic-guards |
| `Pickup restoring {} (its Start / End for a new holder)` | abilities.rs::pickup | Every standard item with onStart / onEnd is one Trick moves (E3), which Pickup's check accepts. | R11-item-restart |
| `Sleep Talk calling {} (its onAfterMove, unchecked for a called move)` | support.rs::sleep_talk_problem (producer) | The standard moves with an onAfterMove are Sparkling Aria and Spit Up (checked for a called move) and Beak Blast, which is `nosleeptalk` and `failcopycat` (E12). | R14-called-multi-hit |
| `Sleep Talk could call {}` | support.rs::sleep_talk_problem (producer) | Forwards the static gate for the user's own moves: no standard move is refused (E1). | R20-generic-guards |
| `Trace copying {} ({})` | switching.rs::trace_copy | Trace copies a standard ability on the field; every one starts and is supported on the field (E1: no ability refused, none switch-in-only). | R1-trace-seeking |
| `Transform copying {} ({})` | transform.rs::transform_into | Transform copies a standard ability on the field; all are supported (E1). | R19-transformed-off-field |
| `Trick moving {} ({})` | moves/handlers.rs::trick | Every standard item with onStart / onEnd moves and the only other onTakeItem items are Mega Stones (E3). | R20-generic-guards |
| `a special mechanic` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `ability {} ending ({})` | switching.rs::end_ability | Every standard ability with onEnd is handled in `end_ability` (E5). | R20-generic-guards |
| `ability {} starting ({})` | switching.rs::start_ability | Every standard ability's start is implemented (E1: none is switch-in-only). | R20-generic-guards |
| `an ability stealing {} ({})` | abilities.rs::steal_item | Pickpocket / Magician: every standard item moves (E3). | R20-generic-guards |
| `callbacks {} are not implemented` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `field effect {}` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `its onAfterMove, unchecked for a called move` | moves/handlers.rs::called_move_problem (producer) | Beak Blast is the only standard move with an unchecked onAfterMove and it is `failcopycat` (E12). | R21-copycat-called-moves |
| `move {}: {}` | support.rs::move_unsupported (producer) | The static gate's wrapper; no standard move is refused (E1). | R20-generic-guards |
| `multi-hit range` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `secondary volatile {}` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `self effect` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `side condition {}` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `side effect #{}` | support.rs::check_state (producer) | The unsupported side effects come only from moves the gate refuses (Pledges, G-Max moves: E1). | R20-generic-guards |
| `slot condition` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `stalling move` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `status move with base power` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `two-turn move` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `volatile {}` | support.rs::move_unsupported (producer) | No standard move is refused (E1). | R20-generic-guards |
| `{} bounced: a multi-hit move` | moves.rs::bounce_move | Magic Bounce reflects status moves, and no standard multi-hit move is a status move (E9). | R20-generic-guards |
| `{} eaten by force` | update.rs::eat_item_forced | Every standard berry's onEat is implemented or empty (E4). | R20-generic-guards |
| `{} eating {}` | moves/handlers.rs::on_hit | Bug Bite / Pluck: every standard berry's onEat is implemented or empty (E4). | R20-generic-guards |
| `{} gaining {} ({})` | abilities.rs::set_ability | `setAbility` gives an ability already on the field (Role Play, Entrainment, Receiver, Mummy) or Simple / Insomnia (Simple Beam, Worry Seed), all supported (E1). | R20-generic-guards |
| `{} moving {} ({})` | moves/handlers.rs::pass_item | Covet / Thief (Bestow is not standard): every standard item moves (E3). | R20-generic-guards |
| `{}: Symbiosis holding {} ({})` | abilities.rs::symbiosis_problem (producer) | Every standard item moves (E3). | R20-generic-guards |
| `{}: ability {} ({})` | support.rs::check_state (producer)<br>mega.rs::mega_target (producer)<br>switching.rs::switch_in_problem (producer) | `check_state`, `mega_target` and `switch_in_problem` refuse an ability `ability_supported_on_field` rejects; every standard ability is supported on the field (E1), Trace too since R1 (its seeking `onUpdate` is `switching::trace_update`), so neither a Trace Mega (Alakazam-Mega, both Meowstic Megas) nor a Trace that never started is refused. | R1-trace-seeking |
| `{}: ability {} switch-in handler ({})` | mod.rs::switching_problem_at_start (producer) | No standard ability is refused only at switch-in (E1). | R20-generic-guards |
| `{}: ability {} switch-in handler {}` | switching.rs::switch_in_problem (producer) | No standard ability is refused only at switch-in (E1). | R20-generic-guards |
| `{}: item {} ({})` | support.rs::check_state (producer)<br>switching.rs::switch_in_problem (producer) | No standard item is refused by the gate (E1). | R20-generic-guards |
| `{}: item {} switch-in handler {}` | mod.rs::switching_problem_at_start (producer)<br>switching.rs::switch_in_problem (producer) | No standard item is refused only at switch-in (E1). | R20-generic-guards |
| `{}: onWeatherChange` | switching.rs::weather_change | Forecast is the only standard onWeatherChange holder and is excluded (E7). | R20-generic-guards |
| `{}: species callbacks` | support.rs::check_state (producer) | No standard species has callbacks (E2). | R20-generic-guards |
| `{}: species callbacks {}` | mega.rs::mega_target (producer) | No standard Mega has species callbacks (E2). | R20-generic-guards |
| `{}: species switch-in handler {}` | mod.rs::switching_problem_at_start (producer)<br>switching.rs::switch_in_problem (producer) | No standard species has callbacks (E2). | R20-generic-guards |

### invariant: a state the engine's own rules never produce

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `Baton Pass passing the {} volatile` | switching.rs::copy_volatile_from | The refused volatiles are locks and charges (Outrage, recharge, two-turn moves, Rollout, Uproar), during which the Pokémon can only use the locked move, and Roost's, which lasts only the turn Roost was used (one action per turn; Sleep Talk needs sleep, which ends `lockedmove` and aborts charges, and Uproar prevents sleep). | R18-baton-pass-volatiles |
| `Skill Swap: {}` | abilities.rs::skill_swap | Symbiosis part: every standard item moves (E3). Rivalry part: the Rivalry holder is on the field before Skill Swap runs, so `check_state` (at the turn start) or `switch_in_problem` (on its switch-in) already refused an undecided gender. | R13-attract-gender |
| `Syrup Bomb's residual with its source neither active nor fainted in place` | conditions.rs::syrup_bomb_residual | A source leaves the field only by a switch or a drag, and every action ends with an Update in which Syrup Bomb's `onUpdate` removes the volatile; a fainted source stays in place until the replacement after the residual. | R12-syrup-bomb-source |
| `Transform by an encored Pokémon (the encored move leaves the move slots)` | transform.rs::transform_into | Transform is `failencore`, `failcopycat`, `nosleeptalk` and `failinstruct`, so an encored Pokémon can only run it by choosing it, and the Champions Encore replaces that queued action with the encored move first (a Mental Herb holder is cured at once); Imposter transforms on switch-in, when Encore is gone (`noCopy`). | R8-encore-edges |
| `{} holding {} has no Mega Evolution` | mega.rs::mega_target (producer) | Mega eligibility comes from the same stone table (`gimmick::structural_gimmicks`), so the ruleset rejects a Mega choice without a Mega Evolution first. | R20-generic-guards |
| `{} with {}` | support.rs::check_state (producer) | Hazard layers and durations as the engine sets them; only a hand-built state differs. | R20-generic-guards |
| `{}: a multi-hit future move` | moves.rs::future_move_hit | The hit loop suspends only between hits of a multi-hit move. The only standard future move is Future Sight (one hit; Doom Desire is not standard, E8), and Parental Bond skips `futuremove` moves. | R5-future-move-edges |
| `{}: damageCallback of {}` | moves.rs::get_damage | Of the standard callbacks, Endeavor runs only when the user has less HP (its onTryImmunity), Super Fang, Metal Burst and Comeuppance return at least 1, Final Gambit the user's HP, and Counter / Mirror Coat 0 only without their condition, when their onTry already failed (Psywave, Nature's Madness, Ruination are not standard). | R20-generic-guards |
| `{}: substitute volatile {} with {} HP` | support.rs::check_state (producer) | The engine sets the volatile and its HP together; only a hand-built state differs. | R20-generic-guards |
| `{}: transformed off the field` | support.rs::check_state (producer) | Leaving the field reverts Transform (`clearVolatile`), so the engine never produces this state; only a hand-built state has it. | R19-transformed-off-field |

### ruleset: excluded by the M-C ruleset (no Tera / Dynamax / Z)

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `{} activation (effects not implemented)` | mod.rs::check_side | The M-C ruleset allows only Mega Evolution; other gimmicks are rejected before (`ActionError`). | R20-generic-guards |
| `{}: Dynamax` | support.rs::check_state (producer) | The M-C ruleset forbids Dynamax. | R20-generic-guards |

### shape: the battle shape (doubles only)

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `Ally Switch in triples` | moves/handlers.rs::on_hit | The engine runs singles and doubles only (`State<1>`, `State<2>`). | R20-generic-guards |

### forward: forwards a producer's message

| 키 | 지점 | 이유 | 보드 |
|---|---|---|---|
| `check_side: -> mega::mega_target(mon)` | mod.rs::check_side | Forwards `mega_target` for a Mega choice; none of its messages is reachable: `{}: ability {} ({})` (every Mega ability is supported, E1; Trace since R1), `{} holding {} has no Mega Evolution` and `{}: species callbacks {}`. | R1-trace-seeking |
| `check_side: -> why` | mod.rs::check_side | Forwards `move_unsupported` (Struggle, the chosen move: E1) and `sleep_talk_problem`, whose remaining messages are unreachable (`Sleep Talk could call {}`: E1; an unchecked onAfterMove: E12). The multi-hit one is fixed (`rr-sleep-talk-multihit`). | R14-called-multi-hit |
| `enumerate_start: -> why` | mod.rs::enumerate_start | Forwards `switching_problem_at_start`: no standard item, species or ability has an unimplemented switch-in handler (E1, E2). | R20-generic-guards |
| `run_mega_evo: -> why` | mega.rs::run_mega_evo | Shadowed: `check_side` runs the same `mega_target` on the chosen Pokémon, and nothing changes its species or item between the choice and the Mega action (Mega Stones cannot be taken). | R20-generic-guards |
| `{} ({})` | moves/handlers.rs::called_move_problem (producer) | `called_move_problem`'s wrapper of its reasons (the move and why); none of them is reachable (see `Copycat calling {}`). | R21-copycat-called-moves |

## 보드 대응

| 보드 | 도달 가능 | 도달 불가능 |
|---|---|---|
| R1-trace-seeking | — | `check_side: -> mega::mega_target(mon)`<br>`Trace next to No Ability`<br>`Trace copying {} (cantsuppress: setAbility fails and Trace keeps seeking)`<br>`Trace copying {} ({})`<br>`{}: ability {} ({})` |
| R2-hazard-effect-order | `{} switching into hazards whose order (Showdown effectOrder) decides the outcome` | — |
| R5-future-move-edges | `{} hitting a holder of {}` | `{}: a multi-hit future move` |
| R5c-future-move-absent-user-occupant | `{} of {} hitting after its user left the field, {}` | — |
| R8-encore-edges | — | `Transform by an encored Pokémon (the encored move leaves the move slots)` |
| R9-beat-up-order | `Beat Up with benched allies of different power {} (their order in Showdown's side.pokemon depends on the switches so far, which the state does not keep)` | — |
| R10-once-per-battle-flags | `{} after the battle start (its once-per-battle flag is not in the state)` | `{}: Battle Bond (its once-per-battle `bondTriggered` is not in the state)` |
| R11-item-restart | — | `Pickup restoring {} (its Start / End for a new holder)` |
| R12-syrup-bomb-source | — | `Syrup Bomb's residual with its source neither active nor fainted in place` |
| R13-attract-gender | `Attract between {} and {} with an undecided gender (give the sets a gender)`<br>`check_turn: -> support::check_state(state)`<br>`{} with {} of undecided gender (give the set a gender)`<br>`switch_in_as: -> why`<br>`{}: Rivalry next to a Pokémon of undecided gender`<br>`Rivalry next to {} of undecided gender (give the set a gender)` | `Skill Swap: {}` |
| R14-called-multi-hit | — | `check_side: -> why`<br>`{} called by {}: a multi-hit called move`<br>`Sleep Talk calling {} (its onAfterMove, unchecked for a called move)` |
| R18-baton-pass-volatiles | — | `Baton Pass passing the {} volatile` |
| R19-transformed-off-field | — | `Transform copying {} ({})`<br>`{}: transformed off the field` |
| R20-generic-guards | — | `{} restarting after Neutralizing Gas at 0 HP`<br>`Gluttony restarting after Neutralizing Gas (its abilityState.gluttony = false)`<br>`Cud Chew eating {}`<br>`two Dancers with the same Speed (Showdown orders them by abilityState.effectOrder)`<br>`{} gaining {} ({})`<br>`an ability stealing {} ({})`<br>`Zygarde-Complete fainting (Power Construct's formeRegression to the set's forme)`<br>`Shields Down on {}: the core colour (the set's species) is not in the state`<br>`Power Construct on {}`<br>`Power Construct making a Zygarde holding {} able to Mega Evolve`<br>`{}: Utility Umbrella's `inactive` item state past the end of a stage (its onUpdate has not run)`<br>`{}: Mirror Herb keeps copied boosts past the end of a stage (its effectState persists until the next trigger)`<br>`run_mega_evo: -> why`<br>`enumerate_start: -> why`<br>`a switch request for {} slot {} (Eject Pack) during {}`<br>`{} activation (effects not implemented)`<br>`Mirror Move calling {}`<br>`Nature Power calling {}`<br>`{} moving {} ({})`<br>`Order Up from a commanded Dondozo whose Tatsugiri is gone (the source of `commanded` is not in the state)`<br>`Relic Song: Meloetta changing forme after fainting`<br>`{} eating {}`<br>`Fling feeding {}`<br>`Ally Switch in triples`<br>`Trick moving {} ({})`<br>`Dancer copying {}: a multi-hit move`<br>`{} bounced: a multi-hit move`<br>`{}: damageCallback of {}`<br>`ability {} starting ({})`<br>`{}: onWeatherChange`<br>`ability {} ending ({})`<br>`{} eaten by force`<br>`field effect #{} (value {})`<br>`field effect #{} without a duration`<br>`side effect #{}`<br>`{} with {}`<br>`{}: item {} ({})`<br>`{}: species callbacks`<br>`{}: Dynamax`<br>`{}: substitute volatile {} with {} HP`<br>`move {}: {}`<br>`callbacks {} are not implemented`<br>`volatile {}`<br>`side condition {}`<br>`field effect {}`<br>`secondary volatile {}`<br>`multi-hit range`<br>`a special mechanic`<br>`stalling move`<br>`two-turn move`<br>`slot condition`<br>`self effect`<br>`status move with base power`<br>`Sleep Talk could call {}`<br>`{} holding {} has no Mega Evolution`<br>`{}: species callbacks {}`<br>`{}: item {} switch-in handler {}`<br>`{}: species switch-in handler {}`<br>`{}: ability {} switch-in handler ({})`<br>`{}: ability {} switch-in handler {}`<br>`{}: Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the suppressor's End WeatherChange)`<br>`{}: Symbiosis holding {} ({})`<br>`Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the suppressor's End WeatherChange)` |
| R21-copycat-called-moves | — | `Copycat calling {}`<br>`{} ({})`<br>`a Rollout lock on the called move`<br>`its onAfterMove, unchecked for a called move` |

보드에 아직 없는 제안 작업(이 감사가 붙인 이름):

- **R21-copycat-called-moves**: Copycat calling a two-turn move (the caller becomes locked into the charge), a move that locks its user (lockedmove) or one that queues its own action (Mirror Coat's beforeTurnCallback, Chilly Reception's priorityChargeCallback) — shares moves::call_move with R14.
- **R22-forced-eat-ignored-item**: Teatime / Stuff Cheeks (eatItem(true)) for a holder that ignores its item (Klutz, Magic Room): the Eat event is skipped, the berry still goes and EatItem still runs; eat_item_forced also never ran EatItem (Cheek Pouch, Cud Chew, Ripen): confirmed by the oracle and fixed in B35 (`oo-teatime-cheek-pouch`, `oo-teatime-cud-chew`, `oo-teatime-ripen`).
- **R5c-future-move-absent-user-occupant**: A future move whose user left the field while the position the engine seats it in holds a Pokémon with onAny/onAlly/onFoe ability or item handlers (Unnerve, Ruin abilities, Battery, Soul-Heart, Opportunist, ...) or a target with Innards Out / Cotton Down / Gulp Missile / Rowap Berry: needs an attacker that is not in any slot (or a narrower, per-event refusal) — repro uu-future-sight-absent-user-unnerve (full: 6).

## 근거 (자동 검사)

| 검사 | 내용 | 결과 |
|---|---|---|
| E1-moves | COVERAGE.md: no move the support gate refuses is in the standard range, and none is refused only at switch-in | 통과: 149 refused, all outside |
| E1-abilities | COVERAGE.md: no ability the support gate refuses is in the standard range, and none is refused only at switch-in (Trace included: its switch-in and, while it seeks, its `onUpdate` are implemented since R1) | 통과: 10 refused, all outside |
| E1-items | COVERAGE.md: no item the support gate refuses is in the standard range, and none is refused only at switch-in | 통과: 86 refused, all outside |
| E2 | No standard species (Megas included) has species callbacks | 통과: 382 species |
| E3 | Every standard item with onStart / onEnd is one `trick_moves_item` moves; the only standard onTakeItem items are Mega Stones | 통과 |
| E4 | Every standard berry's onEat is implemented (`update::berry_on_eat`) or empty (a resist berry) | 통과: 28 berries |
| E5 | Every standard ability with onEnd is handled in `switching::end_ability` | 통과 |
| E6 | Every standard `cantsuppress` ability is also `notrace` | 통과 |
| E7 | The only standard onWeatherChange holder is Forecast | 통과 |
| E8 | Content only non-standard formes have is outside the range: abilities: neutralizinggas, dancer, protosynthesis, quarkdrive, flowergift, powerconstruct, schooling, shieldsdown, commander, noability, deltastream, desolateland, primordialsea, intrepidsword, dauntlessshield, propellertail, iceface, gulpmissile; items: mirrorherb, utilityumbrella, ejectpack, roomservice, boosterenergy, abilityshield, heavydutyboots, figyberry, wikiberry, magoberry, aguavberry, iapapaberry, jabocaberry, rowapberry, custapberry, enigmaberry, micleberry, starfberry, lansatberry, keeberry, marangaberry; moves: mirrormove, naturepower, doomdesire, captivate, bestow, orderup, relicsong, mimic, sketch, metronome, assist, magiccoat, rollout, iceball, shelltrap, revelationdance; species: zygarde, zygardecomplete, zygarde10, minior, meloetta, tatsugiri, dondozo, oricorio, weezinggalar, greninjabond, greninjaash, cherrim, wishiwashi, eiscue, cramorant | 통과 |
| E9 | No standard multi-hit move is a status move (a bounced move never hits twice) and no standard future move is multi-hit | 통과 |
| E10 | No standard move with a beforeTurnCallback / priorityChargeCallback is `failencore` (Encore can lock each); Copycat can call those without `failcopycat` | 통과: beakblast (failcopycat), chillyreception, counter (failcopycat), focuspunch (failcopycat), mirrorcoat |
| E11 | Struggle and Transform are `failinstruct` (Instruct fails on them) | 통과 |
| E12 | Every standard move with an onAfterMove unchecked for a called move (`called_after_move_checked`) is `nosleeptalk` and `failcopycat` | 통과 |
