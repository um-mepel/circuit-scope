// Branching with if/else: tests control-flow lowering.
module ir_branch_if(input a, input b, output reg y);
  always @* begin
    if (a)
      y = b;
    else
      y = ~b;
  end
endmodule

