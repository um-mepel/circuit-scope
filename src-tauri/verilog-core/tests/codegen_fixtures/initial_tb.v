module initial_tb;

reg sel;
reg a;
reg b;
wire y;

assign y = sel ? a : b;

initial begin
    sel = 0;
    a = 0;
    b = 1;
    #10;
    a = 1;
    #10;
    sel = 1;
    #10;
    b = 0;
end

endmodule
