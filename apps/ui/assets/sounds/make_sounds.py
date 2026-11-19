#!/usr/bin/env python3
"""Songbird's own notification sounds, synthesized (no samples, no third-party audio).

Each tone is a few frequency-swept sine "notes" with soft attack and decay, so they read as
short bird-like chirps. Deterministic: running this again writes identical files.
Usage: python3 make_sounds.py  (writes the .wav files next to this script)
"""
import math, os, struct, wave

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))


def note(f0, f1, dur, vol=0.5, vib=0.0):
    n = int(RATE * dur)
    out, phase = [], 0.0
    for i in range(n):
        t = i / n
        f = f0 + (f1 - f0) * t + vib * math.sin(2 * math.pi * 30 * i / RATE)
        phase += 2 * math.pi * f / RATE
        env = min(1.0, t / 0.08) * (1 - t) ** 1.6
        # A little second harmonic for a rounder, whistled timbre.
        out.append(vol * env * (math.sin(phase) + 0.18 * math.sin(2 * phase)))
    return out


def rest(dur):
    return [0.0] * int(RATE * dur)


def write(name, samples):
    peak = max(1e-9, max(abs(s) for s in samples))
    with wave.open(os.path.join(HERE, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(32000 * 0.8 * s / peak)) for s in samples))


TONES = {
    # Message tones: under half a second.
    "chirp": note(2400, 3600, 0.09) + rest(0.03) + note(2600, 3900, 0.11),
    "tweet": note(3200, 2200, 0.14) + rest(0.02) + note(2800, 3400, 0.12),
    "flutter": sum((note(2600 + 120 * k, 3000 + 120 * k, 0.045, vib=60) + rest(0.015) for k in range(5)), []),
    "pip": note(2000, 2100, 0.07) + rest(0.06) + note(2000, 2100, 0.07),
    # Ringtones: about three seconds, played on repeat by the call screen.
    "songbird": sum((note(1800 + 300 * (k % 3), 2600 + 300 * (k % 3), 0.16) + rest(0.06) for k in range(6)), []) + rest(1.6),
    "dawn_chorus": sum((note(2200, 3200, 0.1) + rest(0.04) + note(3000, 2400, 0.14) + rest(0.25) for _ in range(4)), []) + rest(0.8),
    "robin": sum((note(2600 + 400 * math.sin(k), 3400, 0.12, vib=80) + rest(0.08) for k in range(8)), []) + rest(1.1),
    "wren": sum((note(3400, 3800, 0.05) + rest(0.02) for _ in range(14)), []) + rest(1.9),
    "skylark": sum((note(2400 + 90 * k, 3100 + 90 * k, 0.08, vib=120) + rest(0.03) for k in range(12)), []) + rest(1.6),
    "nightingale": sum((note(1600, 1700, 0.25, vib=40) + rest(0.1) + note(2800, 3600, 0.08) + rest(0.3) for _ in range(3)), []) + rest(0.6),
}

if __name__ == "__main__":
    for name, s in TONES.items():
        write(name, s)
