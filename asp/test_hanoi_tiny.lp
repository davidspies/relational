% Tiny Hanoi Tower: 3 disks, 7 steps
% Pegs are 1,2,3,4 (disk 1-4 are pegs)
% Movable disks are 5,6,7 (larger number = smaller disk)

steps(7).
time(0). time(1). time(2). time(3). time(4). time(5). time(6). time(7).

disk(1). disk(2). disk(3). disk(4).  % pegs
disk(5). disk(6). disk(7).           % movable disks

% Initial: all disks on peg 1
on0(5,1).   % largest disk on peg 1
on0(6,5).   % medium disk on largest
on0(7,6).   % smallest disk on medium

% Goal: all disks on peg 3
ongoal(5,3).
ongoal(6,5).
ongoal(7,6).

#show move/2.
