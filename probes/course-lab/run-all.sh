#!/bin/sh
L=./target/release/course-lab.exe
M=../../assets/maps/cleave.map
# Classification boxes + the two delivered-speed gauges. Gauges never affect
# scoring, so adding them leaves each line bit-identical to its earlier run.
G="--gauge HIGH:-896,-448,1920,4864,96,640 \
   --gauge LOW:-448,1216,1920,4864,-32,96 \
   --gauge FARSIDE:-896,-448,3392,3520,96,640 \
   --gauge GAP:-896,-448,2944,3392,-192,768 \
   --gauge lip_leap:-896,-480,2912,2944,96,300 \
   --gauge lip_finish:-320,320,5216,5248,96,300"
BAIL='--forbid-box -320,320,5350,5600,-192,64'
echo "################ PURE LOW ################"
$L $M search --goal finish --waypoint t_cp_fork \
  --via "704,1216,3456,3840,-32,96" --via "-448,320,4352,4864,-32,200" \
  --waypoint t_cp_rejoin --via "-120,120,5100,5248,96,300" \
  --forbid-box "-928,-448,1920,4864,-192,640" $BAIL $G --budget 9000 --fixture --tag low
echo "################ PURE HIGH ################"
$L $M search --goal finish --waypoint t_cp_fork \
  --via "-896,-768,1520,1900,96,300" --via "-880,-500,2700,2944,96,300" \
  --waypoint t_cp_rejoin --via "-120,120,5100,5248,96,300" \
  --forbid-box "-448,1216,1920,4864,-192,384" --forbid-box "-896,-448,2944,3392,-192,100" \
  $BAIL $G --budget 9000 --fixture --tag high
echo "################ DELIBERATE HYBRID ################"
$L $M search --goal finish --waypoint t_cp_fork \
  --via "-896,-768,1520,1900,96,300" --via "-880,-500,2700,2944,96,300" \
  --via "-880,-500,2960,3380,-32,64" --via "704,1216,3456,3840,-32,96" \
  --via "-448,320,4352,4864,-32,200" --waypoint t_cp_rejoin \
  --via "-120,120,5100,5248,96,300" $BAIL $G --budget 9000 --fixture --tag hybrid
