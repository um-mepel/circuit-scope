// Canonicalization + CSE: t1 = a & b, t2 = b & a should merge.
module opt_canonical(input a, input b, output y);
  wire t1, t2;
  assign t1 = a & b;
  assign t2 = b & a;
  assign y = t1 | t2;
endmodule
