// Branching with case: tests multi-way control flow and default.
module ir_branch_case(input [1:0] sel, input [7:0] a, input [7:0] b, output reg [7:0] y);
  always @* begin
    case (sel)
      2'b00: y = a;
      2'b01: y = b;
      2'b10: y = a + b;
      default: y = 8'h00;
    endcase
  end
endmodule

