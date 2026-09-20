#!/bin/bash
# Cross-check our IMBE (TIA-102.BABA) and AMBE+2 decoders against JMBE (a Java decoder) on real speech.
#   ./build_jmbe.sh            (once; needs JMBE_SRC, downloads a JDK and jars into this directory, all git-ignored scratch)
#   ./run_all.sh <bit-error-rate> <outdir>     e.g. ./run_all.sh 0.02 out
# The frame writer is the example `jmbe_frames`; build it first with
#   (cd ../.. && cargo build --release --features ambe_plus_2 --example jmbe_frames)
cd "$(dirname "$0")"; BER=${1:-0.02}; O=${2:-out}; mkdir -p $O
export JAVA_HOME=$(echo $PWD/jdk/jdk-*); export PATH=$JAVA_HOME/bin:$PATH
ROOT=$PWD/../..
(cd $ROOT && for m in imbe a2; do ${CARGO_TARGET_DIR:-target}/release/examples/jmbe_frames $m tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav 800 $OLDPWD/$O/$m $BER 7; done)
R="java -Dorg.slf4j.simpleLogger.defaultLogLevel=error -cp classes:driver_classes:libs/* JmbeDecode"
for v in clean err; do $R imbe $O/imbe.$v.hex $O/imbe.jmbe.$v.raw; $R ambe $O/a2.$v.hex $O/a2.jmbe.$v.raw; done
python3 analyze.py $O
