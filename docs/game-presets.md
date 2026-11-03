# Game presets

eposctl ships four footstep presets: VALORANT, Delta Force, Call of Duty and
PUBG. They are starting points built from the sources below and tuned for the
EPOS H3 series. Nobody has played with them on a real H3PRO yet. Adjust them by
ear: have a friend walk around you in a custom match, or replay a demo.

Values are in dB for the nine bands at 62.5, 125, 250, 500 Hz and 1, 2, 4, 8,
16 kHz. All four turn virtual surround and reverb off and leave the microphone
settings alone.

| Preset | 62.5 | 125 | 250 | 500 | 1k | 2k | 4k | 8k | 16k |
|---|---|---|---|---|---|---|---|---|---|
| VALORANT    | -1.5 | +1.0 | 0    | -1.5 | 0    | +2.0 | +3.0 | +0.5 | 0 |
| Delta Force | -2.0 | 0    | +0.5 | -1.0 | +1.0 | +3.0 | +2.5 | -0.5 | 0 |
| Call of Duty | -3.5 | -1.5 | -1.5 | -0.5 | +0.5 | +3.0 | +3.5 | +1.0 | 0 |
| PUBG        | -3.0 | -1.5 | 0    | 0    | +1.0 | +3.5 | +2.5 | +2.0 | 0 |

## How they were made

No game studio publishes footstep spectra, and the guides found online disagree
with each other. The presets follow what several sources agree on:

- **Cut the low end** (explosions, vehicles, gunfire boom), roughly below 120 Hz.
- **Lift 2-4 kHz**, where the scuff and click of footsteps sit.
- **Small per-game changes** only where sources agree on them.

Each preset was first written as a few parametric filters, converted to the
nine bands with `eposctl eq-import --dry-run`, and then rounded to 0.5 dB.
Peaks that fall between two bands (for example 3 kHz) were raised slightly in
the bands on either side, because sampling at the band centres underestimates
them.

**Adjusted for the headset.** The H3PRO Hybrid has not been measured anywhere
public, and it is not in AutoEq. SoundGuys measured the closely related H3
Hybrid and found sub-bass below 80 Hz, 200-300 Hz and 1-4 kHz "a little
quieter" than their target. Other reviews describe restrained bass. So the
presets lift 1-4 kHz about 1 dB more than the generic advice, and cut the low
end a little less than presets meant for bass-heavy headsets. That the H3PRO
sounds like the H3 Hybrid is an assumption.

**Surround off.** VALORANT, Delta Force and Call of Duty have their own HRTF or
headphone mix. Riot says VALORANT is mixed so footsteps carry over distance,
and that it is mixed in stereo. Adding virtual 7.1 on top can smear those cues.

### Per game

- **VALORANT**: footsteps keep a low thud, so 125 Hz is lifted a little instead
  of cut (eqloadout: +1.5 dB at 200 Hz; Elgato puts them at 100-125 Hz). The
  lift is near 3 kHz, with a small cut at 500 Hz.
- **Delta Force**: sources disagree. English guides cut 60-120 Hz and lift
  1-4 kHz. Chinese guides put running steps at 150-300 Hz, and complain about
  harsh gunfire around 8 kHz. So 250 Hz gets +0.5, 8 kHz gets -0.5, and the
  main lift is at 2-4 kHz.
- **Call of Duty** (Black Ops 6 / Warzone): strong low cut for explosions, and
  the main lift near 2.85 kHz (eqloadout and SteelSeries both name 2.85 kHz).
- **PUBG**: main lift around 2 kHz (eqloadout 2.1 kHz, Elgato 1.9-2.4 kHz). The
  4-8 kHz range is lifted too, because footsteps on grass sit higher than on
  concrete (Hupu).

## Sources

- [eqloadout game presets](https://github.com/luanfernandes/eqloadout/tree/main/presets):
  community Equalizer APO presets for VALORANT, PUBG, Call of Duty and others,
  with their own source list.
- [SoundGuys: EPOS H3 Hybrid review](https://www.soundguys.com/epos-h3-hybrid-review-60307/):
  frequency response measurement of the H3 Hybrid.
- [Tom's Hardware: EPOS H3 Hybrid review](https://www.tomshardware.com/reviews/epos-h3-hybrid-gaming-headset)
  and [PC Perspective: H3PRO Hybrid review](https://pcper.com/2022/07/epos-h3pro-hybrid-wireless-headset-review/):
  listening impressions.
- [Ask VALORANT #6](https://playvalorant.com/en-us/news/game-updates/ask-valorant-6/):
  Riot's audio director on footstep attenuation and the stereo mix.
- [SteelSeries: Sonar EQ for Black Ops 6 and Warzone](https://steelseries.com/blog/best-sonar-eq-settings-to-hear-footsteps-in-black-ops-6-warzone-1174).
- [Elgato EQ guide](https://marketplace.elgato.com/learn/inspiration/elgato-equalizer-step-up-your-game):
  per-game footstep ranges, without a stated method.
- [Immortal Boost: Delta Force settings](https://immortalboost.com/blog/delta-force/settings-guide/),
  [知乎：三角洲脚步声设置](https://zhuanlan.zhihu.com/p/1956032423126075211),
  [什么值得买：三角洲枪声刺耳](https://post.smzdm.com/p/a4628k0k/).
- [虎扑：PUBG 脚步声频率](https://m.hupu.com/bbs/30918899.html) and
  [知乎：PUBG 脚步声与低频](https://www.zhihu.com/question/360096573).
- On real footsteps: Turchet et al.,
  [Footstep sounds synthesis](https://www.researchgate.net/publication/277018835_Footstep_sounds_synthesis_Design_implementation_and_evaluation_of_foot-floor_interactions_surface_materials_shoe_types_and_walkers'_features).
  Footstep sounds are broadband, and harder floors give more high-frequency
  energy. This supports lifting the upper mids rather than one narrow band.
