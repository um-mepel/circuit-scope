// Sequential counter: tests always block, non-blocking assignments, and reset.
module ir_seq_counter(input clk, input rst_n, output [3:0] count);
  reg [3:0] q;

  always @(posedge clk or negedge rst_n) begin
    if (!rst_n)
      q <= 4'd0;
    else
      q <= q + 4'd1;
  end

  assign count = q;
endmodule

