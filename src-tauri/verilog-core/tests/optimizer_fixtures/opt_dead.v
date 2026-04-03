// Dead signal elimination: dead_wire is never used.
module opt_dead(input a, output y);
  wire dead_wire;
  assign dead_wire = 42;
  assign y = a;
endmodule
