# 턴 엔진 데이터 커버리지 (자동 생성)

`cargo run -p lab-scenario --release --bin lab-coverage -- --out engine/COVERAGE.md`로 만든다. `lab-coverage`가 `engine/core/src/turn/support.rs`의 지원 검사를 dex 전체에 적용해 만든다. 사용 횟수는 스캔한 팀 파일 72개(432 세트)에서 센다. 직접 편집하지 않는다.

## 기술

- 전체 938개 중 지원 651개, 등장 효과만 미지원 0개, 미지원 287개.
- 라이브러리 사용 206개 중 지원 201개 (98%).

### 라이브러리에서 쓰이는데 미지원 (사용 횟수순)

| 이름 | 사용 | 상태 | 이유 |
|---|---:|---|---|
| Substitute | 4 | 미지원 | callbacks ["condition.onEnd", "condition.onStart", "condition.onTryPrimaryHit", "onHit", "onTryHit"] are not implemented |
| Double Shock | 3 | 미지원 | callbacks ["onTryMove", "self.onHit"] are not implemented |
| Healing Wish | 3 | 미지원 | callbacks ["condition.onSwap", "condition.onSwitchIn", "onTryHit"] are not implemented |
| Revival Blessing | 3 | 미지원 | callbacks ["onTryHit"] are not implemented |
| Wish | 1 | 미지원 | callbacks ["condition.onEnd", "condition.onResidual", "condition.onStart"] are not implemented |

### 라이브러리에서 쓰이고 지원됨

Protect (201), Fake Out (57), Encore (42), Gravity (37), Heat Wave (34), U-turn (33), Hypnosis (32), Close Combat (29), Grassy Glide (29), Earthquake (28), Ice Beam (27), Recover (27), Hyper Voice (26), Hydro Pump (25), Wood Hammer (25), Rock Slide (24), Trick Room (24), Bite (22), Bullet Seed (22), Population Bomb (22), Quash (22), Dire Claw (21), Swords Dance (21), Fire Blast (19), Flare Blitz (19), Expanding Force (18), Sunny Day (18), Iron Head (17), Moonblast (17), Shadow Ball (17), Taunt (16), Dragon Dance (15), Focus Blast (15), High Horsepower (15), Aqua Jet (14), Liquidation (14), Tailwind (14), Wave Crash (14), Helping Hand (13), Parting Shot (13), Sucker Punch (13), Throat Chop (13), Weather Ball (13), Last Respects (12), Stealth Rock (12), Ice Shard (11), Psychic (11), Roost (11), Dazzling Gleam (10), Follow Me (10), Nasty Plot (10), Aurora Veil (9), Draco Meteor (9), Dragon Rush (9), Dual Wingbeat (9), Flip Turn (9), Glaive Rush (9), Grav Apple (9), Ice Spinner (9), Icicle Crash (9), Make It Rain (9), Thunderbolt (9), Yawn (9), Blizzard (8), Calm Mind (8), Dragon Claw (8), Knock Off (8), Kowtow Cleave (8), Leech Life (8), Solar Beam (8), Double-Edge (7), Hurricane (7), Zap Cannon (7), Armor Cannon (6), Flamethrower (6), Flash Cannon (6), Head Smash (6), Low Kick (6), Sludge Bomb (6), Body Press (5), Dark Pulse (5), Dragon Pulse (5), Extreme Speed (5), Ice Punch (5), Icy Wind (5), Mystical Fire (5), Volt Switch (5), Wide Guard (5), Ancient Power (4), Aura Sphere (4), Earth Power (4), Perish Song (4), Sacred Sword (4), Scale Shot (4), Shadow Claw (4), Shadow Sneak (4), Sparkling Aria (4), Stomping Tantrum (4), Trick (4), Brave Bird (3), Ceaseless Edge (3), Coaching (3), Disable (3), Drain Punch (3), Electro Shot (3), First Impression (3), Freeze-Dry (3), Giga Drain (3), Imprison (3), Leaf Storm (3), Life Dew (3), Play Rough (3), Power Gem (3), Reflect (3), Rock Tomb (3), Scald (3), Slack Off (3), Sleep Powder (3), Terrain Pulse (3), Whirlwind (3), Will-O-Wisp (3), Acrobatics (2), Body Slam (2), Bullet Punch (2), Darkest Lariat (2), Destiny Bond (2), Detect (2), Dragon Tail (2), Drill Run (2), Drum Beating (2), Endure (2), Feint (2), Flower Trick (2), Hex (2), Hyper Beam (2), Infestation (2), Light Screen (2), Light of Ruin (2), Matcha Gotcha (2), Muddy Water (2), Overheat (2), Psychic Fangs (2), Quick Attack (2), Rage Powder (2), Razor Shell (2), Rest (2), Roar (2), Sludge Wave (2), Spiky Shield (2), Strength Sap (2), Surf (2), Thunder Wave (2), Toxic (2), Triple Axel (2), Water Shuriken (2), Air Slash (1), Baneful Bunker (1), Belly Drum (1), Bulk Up (1), Endeavor (1), Fiery Dance (1), Fissure (1), Foul Play (1), Gigaton Hammer (1), Grass Knot (1), Gunk Shot (1), Hammer Arm (1), Haze (1), High Jump Kick (1), Ice Hammer (1), Icicle Spear (1), Iron Defense (1), King's Shield (1), Leech Seed (1), Lumina Crash (1), Mach Punch (1), Memento (1), Metal Burst (1), Misty Terrain (1), Morning Sun (1), Mortal Spin (1), Pain Split (1), Psycho Cut (1), Psyshock (1), Pyro Ball (1), Quiver Dance (1), Rage Fist (1), Rapid Spin (1), Rising Voltage (1), Scorching Sands (1), Shell Smash (1), Snarl (1), Soak (1), Spikes (1), Synthesis (1), Thunder (1), Torch Song (1), Toxic Spikes (1), Twin Beam (1), Vacuum Wave (1), Volt Tackle (1)

### 나머지 미지원 (이유별)

- **미지원: a special mechanic** (41): 10,000,000 Volt Thunderbolt, Acid Downpour, All-Out Pummeling, Black Hole Eclipse, Bloom Doom, Breakneck Blitz, Catastropika, Chloroblast, Clangorous Soulblaze, Continental Crush, Corkscrew Crash, Devastating Drake, Dragon Darts, Extreme Evoboost, Gigavolt Havoc, G-Max Drum Solo, G-Max Fireball, G-Max Gravitas, G-Max Hydrosnipe, G-Max One Blow, G-Max Rapid Flow, G-Max Resonance, Hydro Vortex, Inferno Overdrive, Let's Snuggle Forever, Malicious Moonsault, Menacing Moonraze Maelstrom, Never-Ending Nightmare, Oceanic Operetta, Pulverizing Pancake, Savage Spin-Out, Searing Sunraze Smash, Shattered Psyche, Sinister Arrow Raid, Soul-Stealing 7-Star Strike, Spectral Thief, Stoked Sparksurfer, Subzero Slammer, Supersonic Skystrike, Tectonic Rage, Twinkle Tackle
- **미지원: callbacks ["self.onHit"] are not implemented** (38): G-Max Befuddle, G-Max Centiferno, G-Max Cuddle, G-Max Depletion, G-Max Finale, G-Max Foam Burst, G-Max Gold Rush, G-Max Malodor, G-Max Meltdown, G-Max Replenish, G-Max Sandblast, G-Max Smite, G-Max Stonesurge, G-Max Stun Shock, G-Max Sweetness, G-Max Tartness, G-Max Terror, G-Max Volt Crash, G-Max Wind Rage, Max Airstream, Max Darkness, Max Flare, Max Flutterby, Max Geyser, Max Hailstorm, Max Knuckle, Max Lightning, Max Mindstorm, Max Ooze, Max Overgrowth, Max Phantasm, Max Quake, Max Rockfall, Max Starfall, Max Steelspike, Max Strike, Max Wyrmwind, Sparkly Swirl
- **미지원: callbacks ["onHit"] are not implemented** (28): Acupressure, Assist, Bestow, Block, Camouflage, Conversion, Conversion 2, Copycat, Doodle, Forest's Curse, Freezy Frost, Guard Split, Heal Pulse, Jaw Lock, Magic Powder, Mean Look, Metronome, Mimic, Power Split, Sappy Seed, Sketch, Skill Swap, Spider Web, Thousand Waves, Tidy Up, Transform, Trick-or-Treat, Venom Drench
- **미지원: callbacks ["onBasePower"] are not implemented** (11): Barb Barrage, Brine, Collision Course, Electro Drift, Facade, Fickle Beam, Fusion Bolt, Fusion Flare, Lash Out, Retaliate, Venoshock
- **미지원: callbacks ["onModifyMove"] are not implemented** (8): Bleakwind Storm, Growth, Light That Burns the Sky, Photon Geyser, Present, Sandsear Storm, Secret Power, Wildbolt Storm
- **미지원: callbacks ["onTry"] are not implemented** (7): Belch, Dark Void, Doom Desire, Future Sight, Hyperspace Fury, Last Resort, Teleport
- **미지원: callbacks ["damageCallback"] are not implemented** (5): Guardian of Alola, Nature's Madness, Psywave, Ruination, Super Fang
- **미지원: callbacks ["onHit", "onTryHit"] are not implemented** (5): Autotomize, Entrainment, Mind Reader, Role Play, Simple Beam
- **미지원: callbacks ["onTryHit"] are not implemented** (5): Celebrate, Happy Hour, Mirror Move, Nature Power, Odor Sleuth
- **미지원: callbacks ["onTryMove"] are not implemented** (5): Freeze Shock, Geomancy, Ice Burn, Razor Wind, Skull Bash
- **미지원: callbacks ["secondaries.onHit", "secondary.onHit"] are not implemented** (5): Alluring Voice, Anchor Shot, Burning Jealousy, Eerie Spell, Spirit Shackle
- **미지원: callbacks ["condition.onResidual", "condition.onSideEnd", "condition.onSideStart", "self.onHit"] are not implemented** (4): G-Max Cannonade, G-Max Vine Lash, G-Max Volcalith, G-Max Wildfire
- **미지원: callbacks ["onAfterMoveSecondarySelf"] are not implemented** (4): Fell Stinger, Order Up, Polar Flare, Relic Song
- **미지원: callbacks ["onAfterSubDamage", "onHit"] are not implemented** (4): Core Enforcer, Flame Burst, G-Max Snooze, Splintered Stormshards
- **미지원: callbacks ["onModifyType"] are not implemented** (4): Judgment, Multi-Attack, Revelation Dance, Techno Blast
- **미지원: callbacks ["basePowerCallback"] are not implemented** (3): Pika Papow, Revenge, Veevee Volley
- **미지원: callbacks ["onHitField"] are not implemented** (3): Flower Shield, Rototiller, Teatime
- **미지원: callbacks ["onTryImmunity"] are not implemented** (3): Captivate, Dream Eater, Synchronoise
- **미지원: callbacks ["basePowerCallback", "condition.onResidual", "condition.onStart", "onAfterMove", "onModifyMove"] are not implemented** (2): Ice Ball, Rollout
- **미지원: callbacks ["basePowerCallback", "onHit"] are not implemented** (2): Smelling Salts, Wake-Up Slap
- **미지원: callbacks ["basePowerCallback", "onTryHit"] are not implemented** (2): Heat Crash, Heavy Slam
- **미지원: callbacks ["beforeTurnCallback", "condition.onDamagingHit", "condition.onRedirectTarget", "condition.onStart", "damageCallback", "onTry"] are not implemented** (2): Counter, Mirror Coat
- **미지원: callbacks ["condition.onBasePower", "condition.onFieldEnd", "condition.onFieldStart"] are not implemented** (2): Mud Sport, Water Sport
- **미지원: callbacks ["condition.onCopy", "condition.onEnd", "condition.onRestart", "condition.onStart"] are not implemented** (2): Power Shift, Power Trick
- **미지원: callbacks ["condition.onModifyBoost", "condition.onNegateImmunity", "condition.onStart", "onTryHit"] are not implemented** (2): Foresight, Miracle Eye
- **미지원: callbacks ["condition.onModifyCritRatio", "condition.onStart"] are not implemented** (2): Dragon Cheer, Focus Energy
- **미지원: callbacks ["condition.onResidual", "condition.onStart"] are not implemented** (2): Aqua Ring, Nightmare
- **미지원: callbacks ["condition.onSideStart", "condition.onTryHit", "onTry"] are not implemented** (2): Crafty Shield, Mat Block
- **미지원: callbacks ["onAfterHit"] are not implemented** (2): Covet, Thief
- **미지원: callbacks ["onDamage"] are not implemented** (2): False Swipe, Hold Back
- **미지원: callbacks ["onHit", "onTry"] are not implemented** (2): Stuff Cheeks, Swallow
- **미지원: callbacks ["onHitSide"] are not implemented** (2): Gear Up, Magnetic Flux
- **미지원: callbacks ["onModifyType", "onPrepareHit"] are not implemented** (2): Ivy Cudgel, Natural Gift
- **미지원: callbacks ["onMoveFail"] are not implemented** (2): Mind Blown, Steel Beam
- **미지원: self effect** (2): Baddy Bad, Glitzy Glow
- **미지원: callbacks ["basePowerCallback", "beforeTurnCallback", "condition.onFoeBeforeSwitchOut", "onModifyMove"] are not implemented** (1): Pursuit
- **미지원: callbacks ["basePowerCallback", "condition.onFieldRestart", "condition.onFieldStart", "onTryMove"] are not implemented** (1): Echoed Voice
- **미지원: callbacks ["basePowerCallback", "condition.onModifyMove", "condition.onSideEnd", "condition.onSideStart", "onModifyMove", "onPrepareHit"] are not implemented** (1): Water Pledge
- **미지원: callbacks ["basePowerCallback", "condition.onModifySpe", "condition.onSideEnd", "condition.onSideStart", "onModifyMove", "onPrepareHit"] are not implemented** (1): Grass Pledge
- **미지원: callbacks ["basePowerCallback", "condition.onResidual", "condition.onSideEnd", "condition.onSideStart", "onModifyMove", "onPrepareHit"] are not implemented** (1): Fire Pledge
- **미지원: callbacks ["basePowerCallback", "condition.onRestart", "condition.onStart"] are not implemented** (1): Fury Cutter
- **미지원: callbacks ["basePowerCallback", "onAfterMove", "onTry"] are not implemented** (1): Spit Up
- **미지원: callbacks ["basePowerCallback", "onModifyMove", "onModifyType", "onPrepareHit"] are not implemented** (1): Tera Blast
- **미지원: callbacks ["basePowerCallback", "onModifyMove"] are not implemented** (1): Beat Up
- **미지원: callbacks ["basePowerCallback", "onTry"] are not implemented** (1): Round
- **미지원: callbacks ["beforeMoveCallback", "condition.onBeforeMove", "condition.onDamage", "condition.onEnd", "condition.onMoveAborted", "condition.onStart"] are not implemented** (1): Bide
- **미지원: callbacks ["beforeMoveCallback", "condition.onHit", "condition.onStart", "condition.onTryAddVolatile", "priorityChargeCallback"] are not implemented** (1): Focus Punch
- **미지원: callbacks ["condition.durationCallback", "condition.onBeforeMove", "condition.onDisableMove", "condition.onEnd", "condition.onModifyMove", "condition.onRestart", "condition.onStart", "condition.onTryHeal"] are not implemented** (1): Heal Block
- **미지원: callbacks ["condition.onAccuracy", "condition.onEnd", "condition.onImmunity", "condition.onStart", "condition.onUpdate", "onTry"] are not implemented** (1): Telekinesis
- **미지원: callbacks ["condition.onAccuracy", "condition.onRestart", "condition.onSourceModifyDamage"] are not implemented** (1): Minimize
- **미지원: callbacks ["condition.onAfterMove", "condition.onBasePower", "condition.onEnd", "condition.onMoveAborted", "condition.onRestart", "condition.onStart"] are not implemented** (1): Charge
- **미지원: callbacks ["condition.onAllyTryHitSide", "condition.onStart", "condition.onTryHit"] are not implemented** (1): Magic Coat
- **미지원: callbacks ["condition.onAnyBasePower", "condition.onAnyDragOut", "condition.onAnyInvulnerability", "condition.onFaint", "condition.onFoeBeforeMove", "condition.onFoeTrapPokemon", "condition.onRedirectTarget", "onHit", "onModifyMove", "onMoveFail", "onTry", "onTryHit"] are not implemented** (1): Sky Drop
- **미지원: callbacks ["condition.onAnyPrepareHit", "condition.onStart"] are not implemented** (1): Snatch
- **미지원: callbacks ["condition.onAnySetStatus", "condition.onEnd", "condition.onResidual", "condition.onStart", "onTryHit"] are not implemented** (1): Uproar
- **미지원: callbacks ["condition.onBasePower", "onTryHit"] are not implemented** (1): Me First
- **미지원: callbacks ["condition.onBeforeMove", "condition.onEnd", "condition.onStart", "condition.onUpdate", "onTryImmunity"] are not implemented** (1): Attract
- **미지원: callbacks ["condition.onBeforeMove", "condition.onFaint", "condition.onStart"] are not implemented** (1): Grudge
- **미지원: callbacks ["condition.onBeforeMove", "condition.onHit", "condition.onStart"] are not implemented** (1): Rage
- **미지원: callbacks ["condition.onBeforeMove", "priorityChargeCallback"] are not implemented** (1): Chilly Reception
- **미지원: callbacks ["condition.onCopy", "condition.onStart", "onTryHit"] are not implemented** (1): Gastro Acid
- **미지원: callbacks ["condition.onDragOut", "condition.onResidual", "condition.onStart", "condition.onTrapPokemon"] are not implemented** (1): Ingrain
- **미지원: callbacks ["condition.onEffectiveness", "condition.onStart"] are not implemented** (1): Tar Shot
- **미지원: callbacks ["condition.onEnd", "condition.onImmunity", "condition.onStart", "onTry"] are not implemented** (1): Magnet Rise
- **미지원: callbacks ["condition.onEnd", "condition.onModifyCritRatio", "condition.onRestart", "condition.onStart"] are not implemented** (1): Laser Focus
- **미지원: callbacks ["condition.onEnd", "condition.onResidual", "condition.onStart", "condition.onUpdate"] are not implemented** (1): Syrup Bomb
- **미지원: callbacks ["condition.onEnd", "condition.onResidual", "condition.onStart"] are not implemented** (1): Salt Cure
- **미지원: callbacks ["condition.onEnd", "condition.onRestart", "condition.onStart", "onTry"] are not implemented** (1): Stockpile
- **미지원: callbacks ["condition.onEnd", "condition.onStart"] are not implemented** (1): Embargo
- **미지원: callbacks ["condition.onFieldStart", "condition.onModifyType"] are not implemented** (1): Ion Deluge
- **미지원: callbacks ["condition.onFieldStart", "condition.onTrapPokemon"] are not implemented** (1): Fairy Lock
- **미지원: callbacks ["condition.onHit", "condition.onStart", "onAfterMove", "priorityChargeCallback"] are not implemented** (1): Beak Blast
- **미지원: callbacks ["condition.onHit", "condition.onStart", "onTryMove", "priorityChargeCallback"] are not implemented** (1): Shell Trap
- **미지원: callbacks ["condition.onModifyCritRatio", "condition.onRestart", "condition.onStart", "self.onHit"] are not implemented** (1): G-Max Chi Strike
- **미지원: callbacks ["condition.onModifyType", "condition.onStart", "onTryHit"] are not implemented** (1): Electrify
- **미지원: callbacks ["condition.onResidual", "condition.onStart", "condition.onTrapPokemon", "onTryImmunity"] are not implemented** (1): Octolock
- **미지원: callbacks ["condition.onResidual", "condition.onStart", "onHit", "onModifyMove", "onTryHit"] are not implemented** (1): Curse
- **미지원: callbacks ["condition.onRestart", "condition.onStart", "onHit", "onPrepareHit"] are not implemented** (1): Ally Switch
- **미지원: callbacks ["condition.onRestart", "condition.onStart"] are not implemented** (1): Smack Down
- **미지원: callbacks ["condition.onRestart"] are not implemented** (1): Defense Curl
- **미지원: callbacks ["condition.onSideStart", "condition.onSwitchIn", "self.onHit"] are not implemented** (1): G-Max Steelsurge
- **미지원: callbacks ["condition.onSourceAccuracy", "condition.onSourceInvulnerability", "onHit", "onTryHit"] are not implemented** (1): Lock-On
- **미지원: callbacks ["condition.onStart", "condition.onTryHit", "onHit", "onPrepareHit"] are not implemented** (1): Max Guard
- **미지원: callbacks ["condition.onStart", "condition.onTryMove"] are not implemented** (1): Powder
- **미지원: callbacks ["condition.onSwap", "condition.onSwitchIn", "onTryHit"] are not implemented** (1): Lunar Dance
- **미지원: callbacks ["condition.onUpdate", "onPrepareHit"] are not implemented** (1): Fling
- **미지원: callbacks ["onAfterSubDamage", "onHit", "onModifyMove", "onPrepareHit"] are not implemented** (1): Shell Side Arm
- **미지원: callbacks ["onEffectiveness"] are not implemented** (1): Thousand Arrows
- **미지원: callbacks ["onHit", "onTryHit", "onTryImmunity"] are not implemented** (1): Worry Seed
- **미지원: callbacks ["onHit", "onTryHit", "self.onHit"] are not implemented** (1): Shed Tail
- **미지원: callbacks ["onHit", "self.onHit"] are not implemented** (1): Baton Pass
- **미지원: callbacks ["onModifyMove", "onModifyType"] are not implemented** (1): Tera Starstorm
- **미지원: callbacks ["onModifyMove", "onUseMoveMessage"] are not implemented** (1): Magnitude
- **미지원: callbacks ["onModifyType", "onTry"] are not implemented** (1): Aura Wheel
- **미지원: callbacks ["onTry", "onTryHit"] are not implemented** (1): Splash
- **미지원: callbacks ["onTryHit", "self.onHit"] are not implemented** (1): Psycho Shift
- **미지원: callbacks ["onTryMove", "self.onHit"] are not implemented** (1): Burn Up
- **미지원: callbacks ["secondaries.self.onHit", "secondary.self.onHit"] are not implemented** (1): Genesis Supernova
- **미지원: field effect iondeluge** (1): Plasma Fists
- **미지원: secondary volatile healblock** (1): Psychic Noise

## 특성

- 전체 321개 중 지원 200개, 등장 효과만 미지원 0개, 미지원 121개.
- 라이브러리 사용 66개 중 지원 63개 (95%).

### 라이브러리에서 쓰이는데 미지원 (사용 횟수순)

| 이름 | 사용 | 상태 | 이유 |
|---|---:|---|---|
| Trace | 11 | 미지원 | callbacks ["onStart", "onUpdate"] are not implemented |
| Disguise | 4 | 미지원 | callbacks ["onCriticalHit", "onDamage", "onEffectiveness", "onUpdate"] are not implemented |
| Stance Change | 1 | 미지원 | callbacks ["onModifyMove"] are not implemented |

### 라이브러리에서 쓰이고 지원됨

Intimidate (30), Grassy Surge (28), Competitive (26), Prankster (25), Technician (23), Unburden (22), Unnerve (20), Psychic Surge (16), Thermal Exchange (14), Adaptability (12), Rough Skin (12), Blaze (9), Defiant (9), Good as Gold (9), Hustle (9), Lightning Rod (9), Natural Cure (9), Cursed Body (8), Drizzle (8), Emergency Exit (8), Torrent (8), Flower Veil (7), Rock Head (6), Sand Stream (6), Stamina (6), Flash Fire (5), Snow Warning (5), Chlorophyll (4), Inner Focus (4), Levitate (4), Protean (4), Sharpness (4), Armor Tail (3), Clear Body (3), Pixilate (3), Weak Armor (3), Hospitality (2), Iron Fist (2), Magic Guard (2), Mold Breaker (2), Pressure (2), Regenerator (2), Sand Rush (2), Sturdy (2), Bulletproof (1), Electromorphosis (1), Flame Body (1), Gale Wings (1), Gooey (1), Hyper Cutter (1), Libero (1), Magic Bounce (1), Poison Touch (1), Rain Dish (1), Seed Sower (1), Solar Power (1), Solid Rock (1), Speed Boost (1), Swift Swim (1), Thick Fat (1), Toxic Debris (1), Unaware (1), Volt Absorb (1)

### 나머지 미지원 (이유별)

- **미지원: callbacks ["onModifyAtk", "onModifySpA"] are not implemented** (7): Defeatist, Dragon's Maw, Fire Mane, Rocky Payload, Stakeout, Steelworker, Transistor
- **미지원: callbacks ["onResidual"] are not implemented** (7): Bad Dreams, Harvest, Healer, Hunger Switch, Moody, Pickup, Power Construct
- **미지원: callbacks [] are not implemented** (6): Corrosion, Dancer, Early Bird, Multitype, Persistent, RKS System
- **미지원: callbacks ["onModifyMove"] are not implemented** (5): Infiltrator, Long Reach, Propeller Tail, Stalwart, Stench
- **미지원: callbacks ["onSourceAfterFaint"] are not implemented** (5): Beast Boost, Chilling Neigh, Eelevate, Grim Neigh, Moxie
- **미지원: callbacks ["onBasePower"] are not implemented** (4): Analytic, Flare Boost, Rivalry, Toxic Boost
- **미지원: callbacks ["onAnySetWeather", "onEnd", "onStart"] are not implemented** (3): Delta Stream, Desolate Land, Primordial Sea
- **미지원: callbacks ["onDamagingHit"] are not implemented** (3): Cute Charm, Spicy Spray, Wandering Spirit
- **미지원: callbacks ["onModifyDamage"] are not implemented** (3): Neuroforce, Sniper, Tinted Lens
- **미지원: callbacks ["onAfterMoveSecondary"] are not implemented** (2): Color Change, Pickpocket
- **미지원: callbacks ["onAllyBasePower"] are not implemented** (2): Battery, Power Spot
- **미지원: callbacks ["onAllyFaint"] are not implemented** (2): Power of Alchemy, Receiver
- **미지원: callbacks ["onEnd", "onFoeTryEatItem", "onSourceAfterFaint", "onStart"] are not implemented** (2): As One (Glastrier), As One (Spectrier)
- **미지원: callbacks ["onHitProtect"] are not implemented** (2): Piercing Drill, Unseen Fist
- **미지원: callbacks ["onImmunity", "onModifyAccuracy"] are not implemented** (2): Sand Veil, Snow Cloak
- **미지원: callbacks ["onModifyAccuracy"] are not implemented** (2): Tangled Feet, Wonder Skin
- **미지원: callbacks ["onModifyAtk"] are not implemented** (2): Huge Power, Pure Power
- **미지원: callbacks ["onModifyCritRatio"] are not implemented** (2): Merciless, Super Luck
- **미지원: callbacks ["onModifyDef"] are not implemented** (2): Fur Coat, Grass Pelt
- **미지원: callbacks ["onModifySpA"] are not implemented** (2): Minus, Plus
- **미지원: callbacks ["onModifyWeight"] are not implemented** (2): Heavy Metal, Light Metal
- **미지원: callbacks ["onSwitchIn"] are not implemented** (2): Imposter, Tera Shift
- **미지원: callbacks ["condition.onEnd", "condition.onStart", "onEnd", "onResidual"] are not implemented** (1): Zen Mode
- **미지원: callbacks ["onAfterMoveSecondarySelf"] are not implemented** (1): Magician
- **미지원: callbacks ["onAfterTerastallization"] are not implemented** (1): Teraform Zero
- **미지원: callbacks ["onAllyAfterUseItem"] are not implemented** (1): Symbiosis
- **미지원: callbacks ["onAllyModifyAtk", "onAllyModifySpD", "onStart", "onWeatherChange"] are not implemented** (1): Flower Gift
- **미지원: callbacks ["onAllyTryHitSide", "onTryHit"] are not implemented** (1): Rebound
- **미지원: callbacks ["onAnyAfterMega", "onAnyAfterMove", "onAnyAfterTerastallization", "onAnySwitchIn", "onEnd", "onFoeAfterBoost", "onResidual"] are not implemented** (1): Opportunist
- **미지원: callbacks ["onAnyAfterSetStatus"] are not implemented** (1): Poison Puppeteer
- **미지원: callbacks ["onAnyDamage", "onAnyTryMove"] are not implemented** (1): Damp
- **미지원: callbacks ["onAnyFaint"] are not implemented** (1): Soul-Heart
- **미지원: callbacks ["onAnyModifyAccuracy"] are not implemented** (1): Victory Star
- **미지원: callbacks ["onAnyModifyAtk", "onStart"] are not implemented** (1): Tablets of Ruin
- **미지원: callbacks ["onAnyModifyDef", "onStart"] are not implemented** (1): Sword of Ruin
- **미지원: callbacks ["onAnyModifySpA", "onStart"] are not implemented** (1): Vessel of Ruin
- **미지원: callbacks ["onAnyModifySpD", "onStart"] are not implemented** (1): Beads of Ruin
- **미지원: callbacks ["onAnySwitchIn", "onStart", "onUpdate"] are not implemented** (1): Commander
- **미지원: callbacks ["onBasePower", "onEnd", "onStart"] are not implemented** (1): Supreme Overlord
- **미지원: callbacks ["onBasePower", "onImmunity"] are not implemented** (1): Sand Force
- **미지원: callbacks ["onBeforeMove", "onDisableMove", "onEnd", "onModifyAtk", "onModifyMove", "onStart"] are not implemented** (1): Gorilla Tactics
- **미지원: callbacks ["onBeforeMove", "onStart"] are not implemented** (1): Truant
- **미지원: callbacks ["onBeforeSwitchIn", "onDamagingHit", "onEnd", "onFaint"] are not implemented** (1): Illusion
- **미지원: callbacks ["onChangeBoost", "onEatItem", "onSourceModifyDamage", "onTryEatItem", "onTryHeal"] are not implemented** (1): Ripen
- **미지원: callbacks ["onCriticalHit", "onDamage", "onEffectiveness", "onStart", "onUpdate", "onWeatherChange"] are not implemented** (1): Ice Face
- **미지원: callbacks ["onDamage", "onTryHit"] are not implemented** (1): Mountaineer
- **미지원: callbacks ["onDamage"] are not implemented** (1): Poison Heal
- **미지원: callbacks ["onDamagingHit", "onSourceTryPrimaryHit"] are not implemented** (1): Gulp Missile
- **미지원: callbacks ["onEatItem", "onResidual"] are not implemented** (1): Cud Chew
- **미지원: callbacks ["onEatItem"] are not implemented** (1): Cheek Pouch
- **미지원: callbacks ["onEnd", "onModifyAtk", "onModifySpe", "onResidual", "onStart"] are not implemented** (1): Slow Start
- **미지원: callbacks ["onEnd", "onSwitchIn"] are not implemented** (1): Neutralizing Gas
- **미지원: callbacks ["onFlinch"] are not implemented** (1): Steadfast
- **미지원: callbacks ["onFractionalPriority", "onModifyMove"] are not implemented** (1): Mycelium Might
- **미지원: callbacks ["onFractionalPriority"] are not implemented** (1): Quick Draw
- **미지원: callbacks ["onHit"] are not implemented** (1): Anger Point
- **미지원: callbacks ["onMaybeTrapPokemon", "onTrapPokemon"] are not implemented** (1): Run Away
- **미지원: callbacks ["onModifyAtk", "onStart"] are not implemented** (1): Orichalcum Pulse
- **미지원: callbacks ["onModifyMove", "onSourceAfterFaint"] are not implemented** (1): Battle Bond
- **미지원: callbacks ["onModifySpA", "onStart"] are not implemented** (1): Hadron Engine
- **미지원: callbacks ["onModifySpe"] are not implemented** (1): Surge Surfer
- **미지원: callbacks ["onPrepareHit", "onSourceModifySecondaries"] are not implemented** (1): Parental Bond
- **미지원: callbacks ["onResidual", "onSetStatus", "onStart", "onTryAddVolatile"] are not implemented** (1): Shields Down
- **미지원: callbacks ["onResidual", "onStart"] are not implemented** (1): Schooling
- **미지원: callbacks ["onSourceModifyAccuracy"] are not implemented** (1): Compound Eyes
- **미지원: callbacks ["onSourceTryHeal"] are not implemented** (1): Liquid Ooze
- **미지원: callbacks ["onStart", "onTerrainChange"] are not implemented** (1): Mimicry
- **미지원: callbacks ["onStart", "onWeatherChange"] are not implemented** (1): Forecast
- **미지원: callbacks ["onSwitchIn", "onSwitchOut"] are not implemented** (1): Zero to Hero
- **미지원: callbacks ["onTakeItem"] are not implemented** (1): Sticky Hold
- **미지원: callbacks ["onWeatherModifyDamage"] are not implemented** (1): Mega Sol

## 도구

- 전체 583개 중 지원 466개, 등장 효과만 미지원 0개, 미지원 117개.
- 라이브러리 사용 60개 중 지원 59개 (98%).

### 라이브러리에서 쓰이는데 미지원 (사용 횟수순)

| 이름 | 사용 | 상태 | 이유 |
|---|---:|---|---|
| Miracle Berry | 1 | 미지원 | callbacks ["onEat", "onUpdate"] are not implemented |

### 라이브러리에서 쓰이고 지원됨

Life Orb (52), Sitrus Berry (51), Focus Sash (31), Choice Scarf (24), Leftovers (23), Roseli Berry (22), Miracle Seed (21), Pyroarite (20), Salamencite (15), Psychic Seed (13), Gardevoirite (11), Baxcalibrite (9), Charizardite Y (9), Grassy Seed (9), Starminite (9), Golisopite (8), Black Glasses (7), Floettite (7), Light Clay (7), Raichunite Y (7), Colbur Berry (5), Froslassite (5), Mystic Water (4), Rocky Helmet (4), Garchompite Z (3), Gengarite (3), Metagrossite (3), Swampertite (3), Terrain Extender (3), Air Balloon (2), Chesto Berry (2), Chople Berry (2), Eject Button (2), Electric Seed (2), Fairy Feather (2), Lum Berry (2), Occa Berry (2), Raichunite X (2), Shuca Berry (2), Staraptite (2), Tyranitarite (2), White Herb (2), Aggronite (1), Blastoisinite (1), Cameruptite (1), Charizardite X (1), Crabominite (1), Damp Rock (1), Delphoxite (1), Dragoninite (1), Expert Belt (1), Glimmoranite (1), Iron Ball (1), Kasib Berry (1), Lucarionite Z (1), Passho Berry (1), Sharp Beak (1), Venusaurite (1), Yache Berry (1)

### 나머지 미지원 (이유별)

- **미지원: callbacks ["onBasePower", "onTakeItem"] are not implemented** (24): Adamant Crystal, Cornerstone Mask, Draco Plate, Dread Plate, Earth Plate, Fist Plate, Flame Plate, Griseous Core, Hearthflame Mask, Icicle Plate, Insect Plate, Iron Plate, Lustrous Globe, Meadow Plate, Mind Plate, Pixie Plate, Sky Plate, Splash Plate, Spooky Plate, Stone Plate, Toxic Plate, Vile Vial, Wellspring Mask, Zap Plate
- **미지원: callbacks ["onTakeItem"] are not implemented** (24): Bug Memory, Burn Drive, Chill Drive, Dark Memory, Douse Drive, Dragon Memory, Electric Memory, Fairy Memory, Fighting Memory, Fire Memory, Flying Memory, Ghost Memory, Grass Memory, Ground Memory, Ice Memory, Mail, Poison Memory, Psychic Memory, Rock Memory, Rusted Shield, Rusted Sword, Shock Drive, Steel Memory, Water Memory
- **미지원: callbacks ["onSourceTryPrimaryHit"] are not implemented** (18): Bug Gem, Dark Gem, Dragon Gem, Electric Gem, Fairy Gem, Fighting Gem, Fire Gem, Flying Gem, Ghost Gem, Grass Gem, Ground Gem, Ice Gem, Normal Gem, Poison Gem, Psychic Gem, Rock Gem, Steel Gem, Water Gem
- **미지원: callbacks ["onBasePower"] are not implemented** (8): Adamant Orb, Griseous Orb, Lustrous Orb, Muscle Band, Pink Bow, Polkadot Bow, Soul Dew, Wise Glasses
- **미지원: callbacks ["onModifySpe"] are not implemented** (8): Macho Brace, Power Anklet, Power Band, Power Belt, Power Bracer, Power Lens, Power Weight, Quick Powder
- **미지원: callbacks ["onEat", "onUpdate"] are not implemented** (7): Bitter Berry, Burnt Berry, Ice Berry, Mint Berry, Mystery Berry, PRZ Cure Berry, PSN Cure Berry
- **미지원: callbacks ["onModifyCritRatio"] are not implemented** (3): Leek, Lucky Punch, Stick
- **미지원: callbacks ["onEat", "onResidual", "onTryEatItem"] are not implemented** (2): Berry, Gold Berry
- **미지원: callbacks ["onModifyAccuracy"] are not implemented** (2): Bright Powder, Lax Incense
- **미지원: callbacks ["onSwitchIn", "onTakeItem"] are not implemented** (2): Blue Orb, Red Orb
- **미지원: callbacks ["onUpdate"] are not implemented** (2): Berry Juice, Berserk Gene
- **미지원: callbacks [] are not implemented** (2): Blunder Policy, Ultranecrozium Z
- **미지원: callbacks ["condition.onModifyDamage", "condition.onStart", "condition.onTryMove", "onStart"] are not implemented** (1): Metronome
- **미지원: callbacks ["fling.effect", "onUpdate"] are not implemented** (1): Mental Herb
- **미지원: callbacks ["onAfterBoost", "onAnyAfterMega", "onAnyAfterMove", "onAnySwitchIn", "onEnd", "onResidual", "onUse", "onUseItem"] are not implemented** (1): Eject Pack
- **미지원: callbacks ["onAttract"] are not implemented** (1): Destiny Knot
- **미지원: callbacks ["onBasePower", "onModifyMove"] are not implemented** (1): Punching Glove
- **미지원: callbacks ["onModifyAtk", "onModifySpA"] are not implemented** (1): Light Ball
- **미지원: callbacks ["onModifyAtk"] are not implemented** (1): Thick Club
- **미지원: callbacks ["onModifyDef"] are not implemented** (1): Metal Powder
- **미지원: callbacks ["onModifySpA"] are not implemented** (1): Deep Sea Tooth
- **미지원: callbacks ["onModifySpD"] are not implemented** (1): Deep Sea Scale
- **미지원: callbacks ["onModifyWeight"] are not implemented** (1): Float Stone
- **미지원: callbacks ["onSetAbility"] are not implemented** (1): Ability Shield
- **미지원: callbacks ["onTryBoost"] are not implemented** (1): Clear Amulet
- **미지원: callbacks ["onTryHeal"] are not implemented** (1): Big Root

