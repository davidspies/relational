% Sorting Network Finder (Σ₂ᵖ encoding)
% Problem: ∃ network ∀ input: network sorts input

#const n = 3.
#const k = 4.

wire(0..k-1).
layer(1..n).
bit(0..1).

% =============================================================================
% EXISTENTIAL: Choose comparators
% =============================================================================

{ comp(L, I, X) : wire(I), I < X; comp(L, X, J) : wire(J), X < J } <= 1 :- layer(L); wire(X).

% =============================================================================
% UNIVERSAL: Guess input, verify sorted
% =============================================================================

val(0, W, B) : bit(B) :- wire(W).

left(L, W) :- #count{X : comp(L, W, X)} >= 1; layer(L); wire(W).
right(L, W) :- #count{X : comp(L, X, W)} >= 1; layer(L); wire(W).
touched(L, W) :- #count{left : left(L, W); right : right(L, W)} >= 1; layer(L); wire(W).

val(L, W, V) :- val(L-1, W, V), layer(L), not touched(L, W).

val(L, I, 0) :- left(L, I); val(L-1, I, 0).
val(L, I, 0) :- #count{J : comp(L, I, J), val(L-1, J, 0)} >= 1; layer(L); wire(I).
val(L, I, 1) :- #count{J : comp(L, I, J), val(L-1, I, 1), val(L-1, J, 1)} >= 1; layer(L); wire(I).

val(L, J, 1) :- #count{I : comp(L, I, J), val(L-1, I, 1)} >= 1; layer(L); wire(J).
val(L, J, 1) :- right(L, J), val(L-1, J, 1).
val(L, J, 0) :- #count{I : comp(L, I, J), val(L-1, I, 0), val(L-1, J, 0)} >= 1; layer(L); wire(J).

% =============================================================================
% POSITIVE SORTED CHECK
% Adjacent pair (I,J) is sorted iff val(n,I,0) OR val(n,J,1)
% =============================================================================

sorted_pair(I, I+1) :- #count{left0 : val(n, I, 0); right1 : val(n, I+1, 1)} >= 1; wire(I); wire(I+1).

sorted :- sorted_pair(I, I+1) : wire(I), wire(I+1).

% =============================================================================
% SATURATION
% =============================================================================

val(0, W, B) :- sorted, wire(W), bit(B).

:- not sorted.

#show comp/3.
