// Tests always block parsing and loop unrolling.
module always_loop_test(input clk, input rst, output reg [7:0] data);
  integer i;
  always @(posedge clk) begin
    if (rst) begin
      data <= 0;
    end else begin
      for (i = 0; i < 4; i = i + 1) begin
        data <= data + 1;
      end
    end
  end
endmodule
