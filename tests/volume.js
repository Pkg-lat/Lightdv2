const BASE_URL = 'http://localhost:8070';

// Create volume and test operations
fetch(`${BASE_URL}/volumes`, { method: 'POST' })
  .then(r => r.json())
  .then(vol => {
    console.log('Created:', vol);
    
    // Create folder at root
    return fetch(`${BASE_URL}/volumes/${vol.id}/create-folder`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: '/', name: 'plugins' })
    }).then(r => r.json()).then(() => vol);
  })
  .then(vol => {
    // Write file
    return fetch(`${BASE_URL}/volumes/${vol.id}/write`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        filename: 'hello.txt', 
        content: 'Hello World!' 
      })
    }).then(r => r.json()).then(() => vol);
  })
  .then(vol => {
    // Copy file to plugins folder
    return fetch(`${BASE_URL}/volumes/${vol.id}/copy`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        source: 'hello.txt',
        destination: 'plugins/hello-copy.txt',
        is_folder: false
      })
    }).then(r => r.json()).then(res => {
      console.log('Copied file:', res);
      return vol;
    });
  })
  .then(vol => {
    // Create another folder
    return fetch(`${BASE_URL}/volumes/${vol.id}/create-folder`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: '/', name: 'backup' })
    }).then(r => r.json()).then(() => vol);
  })
  .then(vol => {
    // Copy entire plugins folder to backup
    return fetch(`${BASE_URL}/volumes/${vol.id}/copy`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        source: 'plugins',
        destination: 'backup/plugins',
        is_folder: true
      })
    }).then(r => r.json()).then(res => {
      console.log('Copied folder:', res);
      return vol;
    });
  })
  .then(vol => {
    // Compress files and folders into a zip
    return fetch(`${BASE_URL}/volumes/${vol.id}/compress`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        sources: ['hello.txt', 'plugins'],
        output: 'archive.zip',
        format: 'zip'
      })
    }).then(r => r.json()).then(res => {
      console.log('Compressed to archive:', res);
      return vol;
    });
  })
  .then(vol => {
    // Compress to tar.gz
    return fetch(`${BASE_URL}/volumes/${vol.id}/compress`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        sources: ['backup'],
        output: 'backup.tar.gz',
        format: 'tar.gz'
      })
    }).then(r => r.json()).then(res => {
      console.log('Compressed to tar.gz:', res);
      return vol;
    });
  })
  .then(vol => {
    // List files
    return fetch(`${BASE_URL}/volumes/${vol.id}/files`).then(r => r.json());
  })
  .then(files => console.log('Files:', files))
  .catch(e => console.error('Error:', e));

// List all volumes
setTimeout(() => {
  fetch(`${BASE_URL}/volumes`)
    .then(r => r.json())
    .then(vols => console.log('All volumes:', vols))
    .catch(e => console.error('Error:', e));
}, 1000);
