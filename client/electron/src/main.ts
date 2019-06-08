import { app, BrowserWindow } from "electron";
import * as path from "path";
import

let window: Electron.BrowserWindow

function createWindow() {
  window = new BrowserWindow({
    width: 1200,
    height: 800,
    show: true,
    webPreferences: {
      nodeIntegration: false
    }
  })

  // window.removeMenu()

  window.loadFile(path.join(__dirname, '../index.html'))

  window.on('closed', () => {
    window = null
  })
}

app.on('ready', createWindow)
