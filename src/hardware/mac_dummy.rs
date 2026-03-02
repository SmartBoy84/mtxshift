use crate::hardware::{ButtonMonitor, ButtonMonitorFunctionality, Matrix, MatrixFunctionality};

// dummy implementation to satisfy rust analyzer...
impl MatrixFunctionality for Matrix<()> {
    type Err = ();
    fn new(_din: usize, _cs: usize, _clk: usize) -> Result<Self, Self::Err> {
        panic!("mac os not supported")
    }
    fn clear_display(&mut self, _d: usize) -> Result<(), Self::Err> {
        panic!("mac os not supported")
    }
    fn set_intensity(&mut self, _d: usize, _i: u8) -> Result<(), Self::Err> {
        panic!("mac os not supported")
    }
    fn set_power(&mut self, _state: bool) -> Result<(), Self::Err> {
        panic!("mac os not supported")
    }
    fn write_raw_byte(&mut self, _d: usize, _header: u8, _data: u8) -> Result<(), Self::Err> {
        panic!("mac os not supported")
    }
}

impl ButtonMonitorFunctionality for ButtonMonitor {
    fn monitor(&self) -> impl Future {
        async { panic!("mac os not supported") }
    }
}
