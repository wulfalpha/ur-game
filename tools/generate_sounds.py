"""Generate original Ur effects with deterministic noise and damped resonators."""
import math
import random
import struct
import wave
from pathlib import Path

RATE = 44100
OUT = Path(__file__).resolve().parents[1] / "assets" / "sounds"
OUT.mkdir(parents=True, exist_ok=True)


def render(name, duration, strikes):
    rng = random.Random(4500)
    samples = []
    for i in range(int(RATE * duration)):
        t = i / RATE
        value = 0.0
        for start, freq, decay, gain, noise in strikes:
            age = t - start
            if age >= 0:
                attack = min(1.0, age / 0.002)
                tone = sum(math.sin(2 * math.pi * freq * harmonic * age) / harmonic**2
                           for harmonic in (1, 2, 3))
                value += gain * attack * math.exp(-age / decay) * (
                    tone * (1 - noise) + rng.uniform(-1, 1) * noise)
        # Soft limit peaks and fade the tail to prevent discontinuities.
        value = math.tanh(value) * min(1.0, (duration - t) / 0.025)
        samples.append(struct.pack("<h", int(value * 25000)))
    with wave.open(str(OUT / (name + ".wav")), "wb") as f:
        f.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
        f.writeframes(b"".join(samples))


render("dice", 0.60, [(t, 1100 + i * 137, .022, .42 - i * .025, .72)
                     for i, t in enumerate((0, .045, .105, .18, .28, .40))])
render("move", .18, [(0, 820, .025, .6, .35), (.012, 1640, .018, .16, .15)])
render("capture", .30, [(0, 240, .065, .65, .4), (.025, 510, .04, .35, .3)])
render("rosette", .95, [(i * .10, f, .20, .26, .015)
                       for i, f in enumerate((440, 660, 880))])
render("victory", 1.65, [(i * .15, f, .30, .29, .01)
                        for i, f in enumerate((330, 440, 495, 660, 880))])


render("hop", .10, [(0, 1050, .017, .27, .35)])
render("home", .60, [(0, 660, .10, .30, .04), (.09, 990, .15, .24, .015)])
render("menu", .07, [(0, 1300, .012, .15, .12)])
