#!/bin/sh
# The deliberate hybrid: high opening, drop through the slot on purpose, rejoin
# low. Not a failure path — every waypoint here is chosen.
L=./target/release/course-lab.exe
M=../../assets/maps/cleave.map
$L $M search --goal finish \
  --waypoint t_cp_fork \
  --via "-896,-768,1520,1900,96,300" \
  --via "-880,-500,2700,2944,96,300" \
  --via "-880,-500,2960,3380,-32,64" \
  --via "704,1216,3456,3840,-32,96" \
  --via "-448,320,4352,4864,-32,200" \
  --waypoint t_cp_rejoin \
  --via "-120,120,5100,5248,96,300" \
  --forbid-box "-320,320,5350,5600,-192,64" \
  --gauge lip_leap:-896,-480,2912,2944,96,300 \
  --gauge lip_finish:-320,320,5216,5248,96,300 \
  --budget 9000 --fixture --tag hybrid
