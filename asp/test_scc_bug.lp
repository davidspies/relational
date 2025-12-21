% Minimal example: disjunctive rule with both heads in an SCC
% a | b :- c.
% c :- a.
% c :- b.
% {d}.  % External fact to trigger c

% This creates SCC: {a, b, c}
% Rule "a | b :- c" has both heads in the SCC

a | b :- c.
c :- a.
c :- b.
{d}.
c :- d.
