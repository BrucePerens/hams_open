#!/bin/bash
# JMBE 8ba19fba built with plain javac (no Gradle). JDK = Temurin 21 tarball (system Java is JRE-only); jars from Maven Central.
cd "$(dirname "$0")"; mkdir -p jdk libs classes driver_classes
curl -sfL -o jdk/jdk.tgz "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse" && tar xzf jdk/jdk.tgz -C jdk
M=https://repo1.maven.org/maven2
for u in org/slf4j/slf4j-api/1.7.25/slf4j-api-1.7.25.jar org/slf4j/slf4j-simple/1.7.25/slf4j-simple-1.7.25.jar com/github/wendykierp/JTransforms/3.1/JTransforms-3.1.jar pl/edu/icm/JLargeArrays/1.6/JLargeArrays-1.6.jar org/apache/commons/commons-math3/3.5/commons-math3-3.5.jar; do curl -sfLO --output-dir libs $M/$u; done
export JAVA_HOME=$PWD/jdk/jdk-21.0.12.1+1 PATH=$PWD/jdk/jdk-21.0.12.1+1/bin:$PATH
J=${JMBE_SRC:?set JMBE_SRC to a checkout of github.com/DSheirer/jmbe (commit 8ba19fba)}
javac -nowarn -cp "libs/*" -d classes $(find $J/api/src/main $J/codec/src/main -name "*.java")
javac -cp classes:"libs/*" -d driver_classes JmbeDecode.java
