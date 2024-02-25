#!/bin/bash

# Install packages
sudo sed -i "s/http:\/\/archive.ubuntu.com/http:\/\/mirrors.tuna.tsinghua.edu.cn/g" /etc/apt/sources.list
sudo apt-get update
sudo apt-get install -y build-essential python3-mako

# Create a hypervisor image link to /lib/firmware/rvm-intel.bin
sudo mkdir -p /lib/firmware
sudo ln -sf ~/rvm-intel.bin /lib/firmware

# Clone jailhouse, apply patches and build
git clone -b rt --single-branch https://github.com/rvm-rtos/jailhouse.git
cd jailhouse
make

# Update grub config file to update kernel cmdline
./update-cmdline.sh
sudo update-grub

echo
echo "Setup OK!"
echo "Press ENTER to reboot..."
read
sudo reboot
