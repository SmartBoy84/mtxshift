# mtxshift
Matrix controller with multiple concurrent app support, in Rust.  
This was my initial foray into async Rust - it's pretty cool! I used the smol runtime with the Compat adapter to support gpio_cdev's tokio-based GPIO asynchronous input stream.  

# Setup
The setup is pretty basic:  
PI is connected to a 8x8 matrix and two buttons. One of the two buttons switches between the different apps, and the other button provides interactibility within apps.  

# Apps
## Shift indicator
I use my [Workjam-rs library](https://github.com/SmartBoy84/workjam-rs) to get data for upcoming shifts and display it onto the matrix, periodically updating every five minutes.
Information shown is:
- Calendar-like view of the next three weeks' worth of shifts
- Department I have been assigned to in today's shift
- Time the shift starts (displayed in binary)
- Hearbeat and error indicator bits  
User can press the app button to view the above information about the following days' shifts.  

## Pomodoro timer
Inspired from my [past experiment](https://github.com/SmartBoy84/espmatrix) with embedded-rust, I implemented a pomodoro timer with a configurable time and display type using the button.  
