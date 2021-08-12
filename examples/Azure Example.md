In addition to using a HTTP web page, it is possible to host patches and the UI using an Azure storage account.  This may be desirable if hosting the `rpatchur` in a cloud environment, as it provides the following advantages over a traditional Apache/Nginx webserver virtual machine:
* reduced operating expenses;
  * no compute costs,
  * no virtual network bandwidth costs, and
  * reduced storage costs (no OS disk, more granular storage allocation),
* ability to replicate storage account between availability zones or geographical regions, and
* ability to readily deploy patches with a content delivery network (CDN).

To deploy using an Azure Storage Account, complete the following procedure.
1. Login to the [Azure Portal](https://portal.azure.com).
1. Create an [Azure Storage Account](https://docs.microsoft.com/en-us/azure/storage/common/storage-account-create?tabs=azure-portal).
1. Create a Container within the Storage Account and upload your patches to it.
    1. On the left sidebar navigation plane, under _Data storage_, select _Containers_.
    1. On the top navigation plane, select _+ Container_ and complete the container creation wizard.
    1. Click on the newly created Container to navigate to it.
    1. Click _Upload_ to upload your patches to the Container with your desired file architecture (e.g. nested or flat).
1. Create a Static website within the Storage Account.
    1. On the left sidebar navigation plane, under Data management, select _Static website_.
    1. Toggle the Static website by clicking _Enable_, then click _Save_.
    1. Upload the web content as performed in step 3iv.

![Example Storage Account](https://user-images.githubusercontent.com/50342848/129125580-00e12d96-d6ab-4feb-98b7-c469d4935173.png)

**Create the Patch Container**
![Example Static Website](https://user-images.githubusercontent.com/50342848/129118227-641a9da2-d2e6-4eb3-b33d-09e75d6ce5ed.png)

**Create the Patch Static Website**

