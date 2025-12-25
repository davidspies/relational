#const n = 3.
#const k = 4.

wire(0..k-1).
layer(1..n).

comp(1, 0, 1) :- layer(1); wire(0), wire(1).
{ comp(1, W, W+1) } :- wire(W), wire(W+1), W \ 2 = 0; comp(1, W-2, W-1).
{ comp(L, I, W) : wire(I), I < W; comp(L, W, J) : wire(J), W < J } <= 1 :- layer(L); L > 1; wire(W).

left(L, W) :- comp(L, W, J).
right(L, W) :- comp(L, I, W).
touched(L, W) :- left(L, W).
touched(L, W) :- right(L, W).

:- comp(L, I, J); comp(L-1, I, J).
:- comp(L, I, J); L > 1; not touched(L-1, I); not touched(L-1, J).

val(0, W, 0) | val(0, W, 1) :- wire(W).

val(L, W, V) :- val(L-1, W, V); layer(L); not touched(L, W).
val(L, W, 0) :- left(L, W), val(L-1, W, 0).
val(L, W, 0) :- comp(L, W, J), val(L-1, J, 0).
val(L, W, 0) :- comp(L, I, W), val(L-1, I, 0), val(L-1, W, 0).
val(L, W, 1) :- right(L, W), val(L-1, W, 1).
val(L, W, 1) :- comp(L, I, W), val(L-1, I, 1).
val(L, W, 1) :- comp(L, W, J), val(L-1, W, 1), val(L-1, J, 1).

sorted_pair(I, I+1) :- val(n, I, 0); wire(I); wire(I+1).
sorted_pair(I, I+1) :- val(n, I+1, 1); wire(I); wire(I+1).

sorted :- sorted_pair(I, I+1) : wire(I), wire(I+1).

val(0, W, 0) :- sorted, wire(W).
val(0, W, 1) :- sorted, wire(W).

:- not sorted.

#show comp/3.
