% 3-coloring problem for a small graph
node(1). node(2). node(3). node(4).
edge(1,2). edge(2,3). edge(3,4). edge(4,1). edge(1,3).
color(r). color(g). color(b).

% Each node gets exactly one color
{ col(N,C) : color(C) } = 1 :- node(N).

% Adjacent nodes can't have the same color
:- edge(N1,N2), col(N1,C), col(N2,C).

#show col/2.
