#!/bin/bash
# Creating a replacement file for the watchdog device
sudo touch /var/log/watchdog.log
sudo chmod o+rwx /var/log/watchdog.log